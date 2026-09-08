pub mod dns;
pub mod process;
pub mod routing;
pub mod split;
pub mod ssh_vpn;
pub mod tun;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread;

// ── VPN thread handle (detached, runs until stopped) ─────────────────────────

static VPN_THREAD: Lazy<Mutex<Option<thread::JoinHandle<()>>>> =
    Lazy::new(|| Mutex::new(None));

/// The live split engine, shared with the packet loop and the app poller.
/// `None` while disconnected.
pub static SPLIT: Lazy<Mutex<Option<Arc<Mutex<split::SplitEngine>>>>> =
    Lazy::new(|| Mutex::new(None));

pub fn set_split_engine(engine: Option<Arc<Mutex<split::SplitEngine>>>) {
    *SPLIT.lock() = engine;
}

pub fn split_engine() -> Option<Arc<Mutex<split::SplitEngine>>> {
    SPLIT.lock().clone()
}

/// Push the current settings into a running tunnel. No-op when disconnected —
/// the rules are read fresh at the next connect.
pub fn refresh_split() {
    let Some(engine) = split_engine() else { return };
    let s = crate::config::get_settings();
    engine
        .lock()
        .update_rules(s.split_enabled, s.split_mode, s.rules.clone());
}

/// Routes currently installed by the split engine (0 when disconnected).
pub fn split_route_count() -> usize {
    split_engine().map_or(0, |e| e.lock().installed_count())
}

/// Start VPN in a dedicated OS thread (blocking I/O for WinTun).
pub fn start(cfg: ssh_vpn::SshVpnConfig) {
    let mut guard = VPN_THREAD.lock();
    if guard.is_some() {
        log::warn!("VPN already running");
        return;
    }

    let settings = crate::config::get_settings();
    let rules = settings.rules.clone();
    let enabled = settings.split_enabled;
    let mode = settings.split_mode;

    let handle = thread::spawn(move || {
        ssh_vpn::start_vpn(cfg, rules, enabled, mode);
    });

    *guard = Some(handle);
}

/// Stop the VPN (sends cancellation signal).
pub fn stop() {
    ssh_vpn::stop_vpn();
    let mut guard = VPN_THREAD.lock();
    if let Some(h) = guard.take() {
        // Allow thread to finish (it will clean up routes)
        let _ = h.join();
    }
}

/// Check if VPN thread is still alive.
pub fn is_running() -> bool {
    VPN_THREAD.lock().as_ref().map_or(false, |h| !h.is_finished())
}

/// Get current traffic counters (TX, RX in bytes).
pub fn get_traffic() -> (u64, u64) {
    use std::sync::atomic::Ordering;
    (
        ssh_vpn::TX_BYTES.load(Ordering::Relaxed),
        ssh_vpn::RX_BYTES.load(Ordering::Relaxed),
    )
}
