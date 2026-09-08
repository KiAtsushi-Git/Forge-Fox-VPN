/// WinTun adapter management for Windows.
/// Uses a channel-based interface to bridge blocking WinTun I/O with async code.
use anyhow::{Context, Result};
use std::sync::Arc;

// ── Channel types for async ↔ blocking bridge ─────────────────────────────────
pub struct TunChannels {
    /// Packets coming FROM the TUN (to be sent over SSH)
    pub from_tun: tokio::sync::mpsc::Receiver<Vec<u8>>,
    /// Send packets TO the TUN (received from SSH)
    pub to_tun: tokio::sync::mpsc::Sender<Vec<u8>>,
    /// Close signal (send () to stop the TUN threads)
    pub stop_tx: std::sync::mpsc::Sender<()>,
}

#[cfg(windows)]
pub struct TunAdapter {
    #[allow(dead_code)]
    wintun: wintun::Wintun,
    adapter: Arc<wintun::Adapter>,
}

#[cfg(windows)]
impl TunAdapter {
    /// Create or open the "ForgeFoxVPN" WinTun adapter.
    pub fn new() -> Result<Self> {
        unsafe {
            // 1. Встраиваем wintun.dll в .exe на этапе компиляции
            // Убедись, что путь к wintun.dll правильный относительно файла tun.rs
            let dll_bytes = include_bytes!("../../../wintun.dll");

            // 2. Получаем папку, откуда запущен наш .exe
            let exe_path = std::env::current_exe()?;
            let exe_dir = exe_path.parent().context("Failed to get exe directory")?;
            let dll_path = exe_dir.join("wintun.dll");

            // 3. Выгружаем dll на диск, если её там нет
            if !dll_path.exists() {
                std::fs::write(&dll_path, dll_bytes).context("Failed to extract wintun.dll")?;
            }

            // 4. Загружаем библиотеку
            let wintun = wintun::load().context("Failed to load wintun.dll")?;

            let adapter = match wintun::Adapter::open(&wintun, "ForgeFoxVPN") {
                Ok(a) => {
                    log::info!("Opened existing WinTun adapter");
                    a
                }
                Err(_) => {
                    log::info!("Creating new WinTun adapter");
                    wintun::Adapter::create(&wintun, "ForgeFoxVPN", "ForgeFox", None)
                        .context("Failed to create WinTun adapter")?
                }
            };

            Ok(Self { wintun, adapter })
        }
    }

    /// Configure IP address and MTU, then start a session.
    /// Returns TunChannels for async packet I/O.
    pub fn start(&self, client_ip: &str, mtu: u16, dns_servers: &[String]) -> Result<TunChannels> {
        configure_adapter(client_ip, mtu, dns_servers);

        // Start WinTun session
        let session = self.adapter.start_session(wintun::MAX_RING_CAPACITY)
            .context("Failed to start WinTun session")?;
        let session = Arc::new(session);

        let (from_tun_tx, from_tun_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(512);
        let (to_tun_tx, to_tun_rx)     = tokio::sync::mpsc::channel::<Vec<u8>>(512);
        let (stop_tx, stop_rx)         = std::sync::mpsc::channel::<()>();

        // ── Reader thread: WinTun → from_tun_tx ───────────────────────────────
        let session_r: Arc<wintun::Session> = Arc::clone(&session);
        let from_tun_tx_c = from_tun_tx.clone();
        std::thread::spawn(move || {
            loop {
                if stop_rx.try_recv().is_ok() { break; }
                match session_r.receive_blocking() {
                    Ok(pkt) => {
                        let data = pkt.bytes().to_vec();
                        if from_tun_tx_c.blocking_send(data).is_err() { break; }
                    }
                    Err(e) => {
                        log::warn!("WinTun read error: {}", e);
                        break;
                    }
                }
            }
            log::info!("WinTun reader thread stopped");
        });

        // ── Writer thread: to_tun_rx → WinTun ─────────────────────────────────
        let session_w: Arc<wintun::Session> = Arc::clone(&session);
        let mut to_tun_rx_blocking = to_tun_rx;
        std::thread::spawn(move || {
            while let Some(data) = to_tun_rx_blocking.blocking_recv() {
                match session_w.allocate_send_packet(data.len() as u16) {
                    Ok(mut pkt) => {
                        pkt.bytes_mut().copy_from_slice(&data);
                        session_w.send_packet(pkt);
                    }
                    Err(e) => {
                        log::warn!("WinTun write alloc error: {}", e);
                        break;
                    }
                }
            }
            log::info!("WinTun writer thread stopped");
        });

        log::info!("WinTun session started (client_ip={}, mtu={})", client_ip, mtu);
        Ok(TunChannels { from_tun: from_tun_rx, to_tun: to_tun_tx, stop_tx })
    }
}

#[cfg(windows)]
fn configure_adapter(client_ip: &str, mtu: u16, dns_servers: &[String]) {
    use std::process::Command;
    let _ = Command::new("netsh").args([
        "interface", "ip", "set", "address",
        "name=ForgeFoxVPN", "source=static",
        &format!("addr={}", client_ip), "mask=255.255.255.0",
    ]).output();

    let _ = Command::new("netsh").args([
        "interface", "ipv4", "set", "subinterface",
        "ForgeFoxVPN", &format!("mtu={}", mtu), "store=active",
    ]).output();

    // The split engine routes these same addresses into the tunnel for Proxy
    // mode, so the adapter and the engine have to agree on which resolvers are
    // in use — hardcoding 8.8.8.8 here would silently defeat domain rules
    // whenever the user configured something else.
    let servers: Vec<&String> = dns_servers.iter().filter(|s| !s.trim().is_empty()).collect();
    let fallback = ["8.8.8.8".to_string(), "1.1.1.1".to_string()];
    let servers: Vec<&String> = if servers.is_empty() {
        fallback.iter().collect()
    } else {
        servers
    };

    for (i, srv) in servers.iter().enumerate() {
        let srv = srv.trim();
        if i == 0 {
            let _ = Command::new("netsh").args([
                "interface", "ip", "set", "dns",
                "name=ForgeFoxVPN", "source=static",
                &format!("addr={}", srv), "register=none",
            ]).output();
        } else {
            let _ = Command::new("netsh").args([
                "interface", "ip", "add", "dns",
                "name=ForgeFoxVPN", &format!("addr={}", srv),
                &format!("index={}", i + 1),
            ]).output();
        }
    }
}

// ── Stub for non-Windows ──────────────────────────────────────────────────────
#[cfg(not(windows))]
pub struct TunAdapter;

#[cfg(not(windows))]
impl TunAdapter {
    pub fn new() -> Result<Self> { anyhow::bail!("WinTun is only supported on Windows") }
    pub fn start(&self, _ip: &str, _mtu: u16, _dns: &[String]) -> Result<TunChannels> {
        anyhow::bail!("WinTun is only supported on Windows")
    }
}
