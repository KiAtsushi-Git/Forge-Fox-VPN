/// SSH VPN core — Windows desktop version.
/// Bridges a local WinTun adapter and a remote SSH-based TUN. On the server side
/// it prefers the C bridge (`forgefox-bridge`, installed by install.sh) and falls
/// back to the inline Python script on hosts that have not been upgraded yet.
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use russh::client;
use russh_keys::key::PublicKey;
use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;

// ── Throughput tuning ─────────────────────────────────────────────────────────

/// Сколько пакетов максимум склеиваем в один write в SSH-канал.
/// recv_many не ждёт заполнения буфера — забирает то, что уже в очереди,
/// поэтому батчинг не добавляет задержки на редком трафике.
const TX_BATCH_PACKETS: usize = 64;

/// Буфер чтения из SSH: без него на каждый пакет уходило два read_exact,
/// то есть два прохода по стеку russh вместо копии из готового буфера.
const RX_BUF_BYTES: usize = 256 * 1024;

/// Окно SSH-канала. Пропускная ограничена window/RTT, и стандартные 2 МБ
/// при RTT 60 мс дают ~266 Мбит. Влияет на приём (сервер→клиент): сколько
/// сервер вправе прислать без подтверждения. Обратное направление задаёт
/// sshd своим окном, его из клиента не поднять.
const CHANNEL_WINDOW: u32 = 16 * 1024 * 1024;

/// AES-GCM вперёд chacha20: на x86 с AES-NI он заметно быстрее, а именно
/// шифрование становится узким местом после снятия syscall-накладных.
const FAST_CIPHERS: &[russh::cipher::Name] = &[
    russh::cipher::AES_256_GCM,
    russh::cipher::CHACHA20_POLY1305,
    russh::cipher::AES_256_CTR,
];

// ── Public config ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SshVpnConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// IP assigned to the local WinTun adapter (e.g. "10.42.7.2")
    pub client_ip: String,
    /// IP that the Python script will configure on the server-side TUN (e.g. "10.42.7.1")
    pub server_tun_ip: String,
    pub mtu: u16,
}

// ── Traffic counters ──────────────────────────────────────────────────────────

pub static TX_BYTES: AtomicU64 = AtomicU64::new(0);
pub static RX_BYTES: AtomicU64 = AtomicU64::new(0);

pub fn reset_counters() {
    TX_BYTES.store(0, Ordering::Relaxed);
    RX_BYTES.store(0, Ordering::Relaxed);
}

// ── Cancellation token ────────────────────────────────────────────────────────

static CANCEL_TX: Lazy<Mutex<Option<broadcast::Sender<()>>>> =
    Lazy::new(|| Mutex::new(None));

pub fn send_cancel() {
    if let Some(tx) = CANCEL_TX.lock().take() {
        let _ = tx.send(());
    }
}

// ── Log buffer (polled by frontend via get_logs command) ──────────────────────

const MAX_LOG_LINES: usize = 500;
static LOG_BUFFER: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

pub fn get_logs() -> Vec<String> {
    LOG_BUFFER.lock().clone()
}

pub fn clear_logs() {
    LOG_BUFFER.lock().clear();
}

pub fn vpn_log_ext(msg: impl Into<String>) {
    vpn_log(msg);
}

fn vpn_log(msg: impl Into<String>) {
    let msg = msg.into();
    log::info!("[VPN] {}", msg);
    let mut buf = LOG_BUFFER.lock();
    buf.push(msg);
    if buf.len() > MAX_LOG_LINES {
        buf.remove(0);
    }
}

// ── SSH client handler ────────────────────────────────────────────────────────

struct ClientHandler;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true) // Accept all server keys (TODO: known_hosts support)
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Blocking entry-point — call from a dedicated OS thread.
pub fn start_vpn(
    cfg: SshVpnConfig,
    rules: Vec<crate::config::SplitRule>,
    split_enabled: bool,
    mode: crate::config::SplitMode,
) {
    reset_counters();
    clear_logs();
    vpn_log(format!("Connecting to {}:{} as {}…", cfg.host, cfg.port, cfg.username));

    // ── Create WinTun adapter ─────────────────────────────────────────────────
    let tun = match super::tun::TunAdapter::new() {
        Ok(t) => t,
        Err(e) => { vpn_log(format!("TUN create error: {e}")); return; }
    };

    // Read once and share: the adapter's resolvers and the engine's resolver
    // routes must be the same set or Proxy-mode domain rules learn nothing.
    let dns_servers = crate::config::get_settings().dns_servers.clone();

    let tun_channels = match tun.start(&cfg.client_ip, cfg.mtu, &dns_servers) {
        Ok(ch) => ch,
        Err(e) => { vpn_log(format!("TUN start error: {e}")); return; }
    };

    vpn_log(format!("WinTun adapter up — client IP {}", cfg.client_ip));

    // ── Route setup ───────────────────────────────────────────────────────────
    let real_gw = match super::routing::get_default_gateway() {
        Ok(gw) => gw,
        Err(e) => { vpn_log(format!("Cannot find default gateway: {e}")); return; }
    };
    vpn_log(format!("Real gateway: {real_gw}"));

    // Must come before any tunnel route: the SSH connection itself has to keep
    // using the physical link or the tunnel would carry its own transport.
    super::routing::add_host_route(&cfg.host, &real_gw);

    // The split engine owns the default route as well — Proxy mode intentionally
    // does not install one.
    let engine = Arc::new(parking_lot::Mutex::new(super::split::SplitEngine::new(
        mode,
        split_enabled,
        rules,
        real_gw.clone(),
        cfg.server_tun_ip.clone(),
        dns_servers.clone(),
    )));
    engine.lock().activate();
    super::set_split_engine(Some(Arc::clone(&engine)));

    // ── Run async tunnel on a tokio runtime ───────────────────────────────────
    // Многопоточный рантайм: на 500 Мбит шифрование, кадрирование и разбор
    // пакетов не помещаются в одно ядро, а current_thread их туда и сводил.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("tokio runtime");

    let (cancel_tx, mut cancel_rx) = broadcast::channel::<()>(1);
    *CANCEL_TX.lock() = Some(cancel_tx);

    let cfg_clone = cfg.clone();
    let super::tun::TunChannels { from_tun, to_tun, stop_tx: tun_stop } = tun_channels;
    let from_tun = Arc::new(tokio::sync::Mutex::new(from_tun));

    let engine_pkt = Arc::clone(&engine);
    let engine_poll = Arc::clone(&engine);

    rt.block_on(async move {
        // Watch running processes for per-app rules. One cheap syscall a second.
        let app_poller = tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tick.tick().await;
                engine_poll.lock().poll_apps();
            }
        });

        tokio::select! {
            _ = cancel_rx.recv() => {
                vpn_log("VPN cancelled by user.");
            }
            _ = run_ssh_with_reconnect(cfg_clone, from_tun, to_tun, engine_pkt) => {}
        }
        app_poller.abort();
        let _ = tun_stop.send(());
    });

    // ── Cleanup routes ────────────────────────────────────────────────────────
    super::set_split_engine(None);
    engine.lock().deactivate();
    super::routing::del_host_route(&cfg.host);

    vpn_log("VPN stopped — routes cleaned up.");
}

pub fn stop_vpn() {
    send_cancel();
}

// ── Reconnect loop ────────────────────────────────────────────────────────────

async fn run_ssh_with_reconnect(
    cfg: SshVpnConfig,
    from_tun: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>,
    to_tun: tokio::sync::mpsc::Sender<Vec<u8>>,
    engine: Arc<parking_lot::Mutex<crate::vpn::split::SplitEngine>>,
) {
    loop {
        match run_ssh_once(
            &cfg,
            Arc::clone(&from_tun),
            to_tun.clone(),
            Arc::clone(&engine),
        )
        .await
        {
            Ok(_) => {
                vpn_log("SSH session ended normally.");
                break;
            }
            Err(e) => {
                vpn_log(format!("SSH error: {e}. Reconnecting in 3s…"));
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                if CANCEL_TX.lock().is_none() { break; }
            }
        }
    }
}

// ── Core SSH tunnel ───────────────────────────────────────────────────────────

async fn run_ssh_once(
    cfg: &SshVpnConfig,
    from_tun: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>,
    to_tun: tokio::sync::mpsc::Sender<Vec<u8>>,
    engine: Arc<parking_lot::Mutex<crate::vpn::split::SplitEngine>>,
) -> Result<()> {
    use tokio::net::TcpStream;

    let addr = format!("{}:{}", cfg.host, cfg.port);
    vpn_log(format!("Connecting to {addr}…"));

    let stream = TcpStream::connect(&addr).await
        .with_context(|| format!("TCP connect to {addr} failed"))?;

    // Пакеты и так уходят батчами; ждать Нейгла незачем — это чистая задержка.
    let _ = stream.set_nodelay(true);

    let mut config = client::Config::default();
    config.window_size = CHANNEL_WINDOW;
    config.maximum_packet_size = 32768;
    config.preferred.cipher = Cow::Borrowed(FAST_CIPHERS);
    // Присваиваем поля напрямую: Limits::new() ассертит потолок 1<<30, а на
    // 500 Мбит рекей раз в гигабайт — это раз в ~17 секунд, с паузой каждый раз.
    config.limits.rekey_write_limit = 16 << 30;
    config.limits.rekey_read_limit = 16 << 30;
    config.limits.rekey_time_limit = std::time::Duration::from_secs(3600);

    let config = Arc::new(config);
    let mut session = client::connect_stream(config, stream, ClientHandler).await
        .context("SSH handshake failed")?;

    vpn_log("SSH connected — authenticating…");

    let auth_ok = session
        .authenticate_password(cfg.username.clone(), cfg.password.clone())
        .await
        .context("SSH auth error")?;

    if !auth_ok {
        bail!("SSH password authentication rejected");
    }

    vpn_log("SSH authenticated — opening channel…");

    let channel = session
        .channel_open_session()
        .await
        .context("channel_open_session failed")?;

    // Серверный мост.
    //
    // Быстрый путь — forgefox-bridge (C, два потока, батчинг пакетов в один
    // write), его ставит install.sh. Если сервер ещё не обновлён, откатываемся
    // на встроенный Python-скрипт: он медленный (~15-25 Мбит упирается в GIL и
    // ~5 syscall'ов на пакет), но рабочий, поэтому старые сервера не ломаются.
    //
    // Кроме кадрирования мост обязан настроить сервер как роутер: поднять TUN,
    // включить forwarding и заNATить туннельную подсеть в физический интерфейс.
    // Без NAT ответы не возвращаются, и туннель выглядит поднятым, но пустым.
    //
    // Правила iptables ставятся с guard'ом `-C … ||`, чтобы переподключение не
    // плодило дубликаты; на выходе они остаются — их может использовать
    // параллельная сессия.
    let python_script = format!(
"python3 -u -c \"
import os,struct,fcntl,sys,select
try:
 tun=open('/dev/net/tun','r+b',buffering=0)
 req=bytearray(b'\\x00'*16+struct.pack('H',0x1001))
 fcntl.ioctl(tun,0x400454ca,req)
 i=req[:16].strip(b'\\x00').decode('ascii')
 os.system('ip link set '+i+' up && ip addr replace {server_ip}/24 dev '+i+' && ip link set dev '+i+' mtu {mtu}')
 net='.'.join('{server_ip}'.split('.')[:3])+'.0/24'
 os.system('sysctl -w net.ipv4.ip_forward=1 >/dev/null 2>&1')
 r=os.popen('ip route show default').read().split()
 d=r[r.index('dev')+1] if 'dev' in r else ''
 if d:
  os.system('iptables -t nat -C POSTROUTING -s '+net+' -o '+d+' -j MASQUERADE 2>/dev/null || iptables -t nat -A POSTROUTING -s '+net+' -o '+d+' -j MASQUERADE')
  os.system('iptables -C FORWARD -i '+i+' -j ACCEPT 2>/dev/null || iptables -I FORWARD 1 -i '+i+' -j ACCEPT')
  os.system('iptables -C FORWARD -o '+i+' -j ACCEPT 2>/dev/null || iptables -I FORWARD 1 -o '+i+' -j ACCEPT')
  print('nat ready via '+d,file=sys.stderr)
 else:
  print('no default route on server, NAT not configured',file=sys.stderr)
 def rx(f,n):
  b=b''
  while len(b)<n:
   c=f.read(n-len(b))
   if not c:return None
   b+=c
  return b
 while True:
  r,_,_=select.select([sys.stdin.buffer,tun],[],[])
  if sys.stdin.buffer in r:
   lb=rx(sys.stdin.buffer,2)
   if not lb:break
   l,=struct.unpack('!H',lb)
   p=rx(sys.stdin.buffer,l)
   if not p:break
   tun.write(p)
  if tun in r:
   p=tun.read(4096)
   sys.stdout.buffer.write(struct.pack('!H',len(p))+p)
   sys.stdout.buffer.flush()
except Exception as e:
 import sys; print(str(e),file=sys.stderr)
\"",
        server_ip = cfg.server_tun_ip,
        mtu = cfg.mtu,
    );

    let bridge_cmd = format!(
        "if command -v forgefox-bridge >/dev/null 2>&1; then exec forgefox-bridge {server_ip} {mtu}; else {python_script}; fi",
        server_ip = cfg.server_tun_ip,
        mtu = cfg.mtu,
        python_script = python_script,
    );

    channel.exec(true, bridge_cmd.into_bytes()).await
        .context("Failed to exec server bridge")?;

    vpn_log(format!(
        "Tunnel active — client={}, server={}",
        cfg.client_ip, cfg.server_tun_ip
    ));

    let (ssh_rx, mut ssh_tx) = tokio::io::split(channel.into_stream());

    // Task A: TUN → SSH.
    //
    // Кадры собираются в один буфер и уходят одним write_all. Раньше на каждый
    // IP-пакет приходилось два write_all — двухбайтовый заголовок улетал
    // отдельным SSH_MSG_CHANNEL_DATA со своими ~30 байтами обвязки — плюс
    // flush, который у russh no-op (ChannelTx::poll_flush сразу Ready(Ok)).
    //
    // write_all сам нарежет буфер: poll_write клампит запись по
    // maximum_packet_size и окну и возвращает частичный результат.
    let tun_to_ssh = tokio::spawn(async move {
        let mut from_tun = from_tun.lock().await;
        let mut batch: Vec<Vec<u8>> = Vec::with_capacity(TX_BATCH_PACKETS);
        let mut out: Vec<u8> = Vec::with_capacity(TX_BATCH_PACKETS * 1600);
        loop {
            batch.clear();
            // Ждёт первый пакет, дальше добирает уже накопленные без ожидания.
            if from_tun.recv_many(&mut batch, TX_BATCH_PACKETS).await == 0 {
                break;
            }
            out.clear();
            let mut sent = 0u64;
            for pkt in &batch {
                let len = pkt.len();
                if len == 0 || len > u16::MAX as usize { continue; }
                out.extend_from_slice(&(len as u16).to_be_bytes());
                out.extend_from_slice(pkt);
                sent += len as u64;
            }
            if out.is_empty() { continue; }
            if ssh_tx.write_all(&out).await.is_err() { break; }
            TX_BYTES.fetch_add(sent, Ordering::Relaxed);
        }
    });

    // Task B: SSH → TUN  (read framed packets from SSH, send to tun sender)
    let ssh_to_tun = tokio::spawn(async move {
        // Без буфера каждый пакет стоил два прохода по стеку russh.
        let mut ssh_rx = BufReader::with_capacity(RX_BUF_BYTES, ssh_rx);
        let mut len_buf = [0u8; 2];
        let mut pkt_buf = vec![0u8; 65536];
        loop {
            if ssh_rx.read_exact(&mut len_buf).await.is_err() { break; }
            let len = u16::from_be_bytes(len_buf) as usize;
            if len == 0 || len > pkt_buf.len() { break; }
            if ssh_rx.read_exact(&mut pkt_buf[..len]).await.is_err() { break; }
            let data = pkt_buf[..len].to_vec();
            RX_BYTES.fetch_add(len as u64, Ordering::Relaxed);

            // Inbound DNS answers are what keep domain / domain-zone rules
            // current: CDNs hand out different addresses per lookup, so a
            // one-shot resolve at connect time goes stale fast.
            engine.lock().observe_packet(&data);

            if to_tun.send(data).await.is_err() { break; }
        }
    });

    let _ = tokio::try_join!(tun_to_ssh, ssh_to_tun);
    vpn_log("SSH bridge loop ended.");
    Ok(())
}
