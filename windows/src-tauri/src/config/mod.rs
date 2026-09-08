use std::path::PathBuf;
use anyhow::Result;
use once_cell::sync::Lazy;
use parking_lot::RwLock;

pub mod models;
pub use models::*;

// ── Persistent config paths ───────────────────────────────────────────────────

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ForgeFoxVPN")
}

fn settings_path() -> PathBuf { config_dir().join("settings.json") }
fn servers_path() -> PathBuf  { config_dir().join("servers.json") }

// ── In-memory state (RwLock for concurrent read/write) ────────────────────────

pub static SETTINGS: Lazy<RwLock<AppSettings>> = Lazy::new(|| {
    RwLock::new(load_settings_from_disk().unwrap_or_default())
});

pub static SERVERS: Lazy<RwLock<ServersStore>> = Lazy::new(|| {
    RwLock::new(load_servers_from_disk().unwrap_or_default())
});

// ── Load helpers ──────────────────────────────────────────────────────────────

fn load_settings_from_disk() -> Result<AppSettings> {
    let data = std::fs::read_to_string(settings_path())?;
    Ok(serde_json::from_str(&data)?)
}

fn load_servers_from_disk() -> Result<ServersStore> {
    let data = std::fs::read_to_string(servers_path())?;
    Ok(serde_json::from_str(&data)?)
}

// ── Save helpers ──────────────────────────────────────────────────────────────

pub fn save_settings(s: &AppSettings) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(s)?;
    std::fs::write(settings_path(), json)?;
    Ok(())
}

pub fn save_servers(s: &ServersStore) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(s)?;
    std::fs::write(servers_path(), json)?;
    Ok(())
}

// ── Public API used by commands.rs ────────────────────────────────────────────

pub fn get_settings() -> AppSettings { SETTINGS.read().clone() }

pub fn update_settings(f: impl FnOnce(&mut AppSettings)) {
    let mut s = SETTINGS.write();
    f(&mut s);
    let _ = save_settings(&s);
}

pub fn get_servers() -> ServersStore { SERVERS.read().clone() }

pub fn update_servers(f: impl FnOnce(&mut ServersStore)) {
    let mut s = SERVERS.write();
    f(&mut s);
    let _ = save_servers(&s);
}
