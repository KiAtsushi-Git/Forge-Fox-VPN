/// Per-application traffic discovery on Windows.
///
/// True per-process routing needs a WFP callout driver (kernel mode). Instead we
/// watch the kernel's TCP connection table, attribute each connection to the PID
/// that owns it, and install a /32 route for the remote address of any connection
/// belonging to a listed executable.
///
/// Known limits, by design:
///   * A connection is caught on the next poll tick, so its first packets may
///     take the default path.
///   * UDP (and therefore QUIC) carries no connection state to enumerate, so
///     UDP-only apps are not covered. Browsers fall back to TCP when QUIC fails,
///     but a QUIC-only app will not be matched.
///   * A remote IP shared by a listed and an unlisted app is routed for both.
use std::collections::HashMap;
use std::net::Ipv4Addr;

#[derive(Debug, Clone)]
pub struct AppConn {
    pub pid: u32,
    pub remote: Ipv4Addr,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunningApp {
    pub pid: u32,
    /// Bare file name, lowercased ("chrome.exe")
    pub name: String,
    /// Full path when obtainable, else empty
    pub path: String,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::ffi::OsStringExt;

    // ── Minimal Win32 bindings (avoids pulling extra crate features) ──────────

    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(
            pTcpTable: *mut std::ffi::c_void,
            pdwSize: *mut u32,
            bOrder: i32,
            ulAf: u32,
            TableClass: u32,
            Reserved: u32,
        ) -> u32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> isize;
        fn CloseHandle(hObject: isize) -> i32;
        fn QueryFullProcessImageNameW(
            hProcess: isize,
            dwFlags: u32,
            lpExeName: *mut u16,
            lpdwSize: *mut u32,
        ) -> i32;
    }

    #[link(name = "psapi")]
    extern "system" {
        fn EnumProcesses(lpidProcess: *mut u32, cb: u32, lpcbNeeded: *mut u32) -> i32;
    }

    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_ALL: u32 = 5;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const NO_ERROR: u32 = 0;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    #[repr(C)]
    struct MibTcpRowOwnerPid {
        state: u32,
        local_addr: u32,
        local_port: u32,
        remote_addr: u32,
        remote_port: u32,
        owning_pid: u32,
    }

    /// Snapshot every IPv4 TCP connection with a real remote endpoint.
    pub fn list_tcp_connections() -> Vec<AppConn> {
        let mut size: u32 = 0;
        // First call: ask for the required buffer size.
        let rc = unsafe {
            GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut size,
                0,
                AF_INET,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if rc != ERROR_INSUFFICIENT_BUFFER && rc != NO_ERROR {
            return Vec::new();
        }
        if size == 0 {
            return Vec::new();
        }

        // Over-allocate slightly: the table can grow between the two calls.
        size = size.saturating_add(4096);
        let mut buf = vec![0u8; size as usize];
        let rc = unsafe {
            GetExtendedTcpTable(
                buf.as_mut_ptr() as *mut std::ffi::c_void,
                &mut size,
                0,
                AF_INET,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if rc != NO_ERROR {
            return Vec::new();
        }

        if buf.len() < 4 {
            return Vec::new();
        }
        let entries = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let row_size = std::mem::size_of::<MibTcpRowOwnerPid>();
        let mut out = Vec::with_capacity(entries.min(4096));

        for i in 0..entries {
            let off = 4 + i * row_size;
            if off + row_size > buf.len() {
                break;
            }
            // SAFETY: offset bounds-checked above; MibTcpRowOwnerPid is repr(C)
            // and matches MIB_TCPROW_OWNER_PID exactly.
            let row = unsafe { &*(buf.as_ptr().add(off) as *const MibTcpRowOwnerPid) };
            if row.owning_pid == 0 || row.remote_addr == 0 {
                continue;
            }
            // remote_addr is in network byte order.
            let remote = Ipv4Addr::from(u32::from_be(row.remote_addr));
            if remote.is_loopback() || remote.is_unspecified() || remote.is_broadcast() {
                continue;
            }
            out.push(AppConn { pid: row.owning_pid, remote });
        }
        out
    }

    /// Full image path for a PID. Empty string when access is denied
    /// (system processes) or the process already exited.
    pub fn process_path(pid: u32) -> String {
        if pid == 0 {
            return String::new();
        }
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h == 0 {
                return String::new();
            }
            let mut buf = vec![0u16; 1024];
            let mut len: u32 = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(h);
            if ok == 0 || len == 0 {
                return String::new();
            }
            std::ffi::OsString::from_wide(&buf[..len as usize])
                .to_string_lossy()
                .into_owned()
        }
    }

    /// Every PID currently on the system (for the UI app picker).
    pub fn enum_pids() -> Vec<u32> {
        let mut pids = vec![0u32; 4096];
        let mut needed: u32 = 0;
        let ok = unsafe {
            EnumProcesses(
                pids.as_mut_ptr(),
                (pids.len() * std::mem::size_of::<u32>()) as u32,
                &mut needed,
            )
        };
        if ok == 0 {
            return Vec::new();
        }
        let count = needed as usize / std::mem::size_of::<u32>();
        pids.truncate(count.min(pids.len()));
        pids
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn list_tcp_connections() -> Vec<AppConn> { Vec::new() }
    pub fn process_path(_pid: u32) -> String { String::new() }
    pub fn enum_pids() -> Vec<u32> { Vec::new() }
}

pub use imp::{enum_pids, list_tcp_connections, process_path};

/// Bare lowercased file name from a full path.
pub fn file_name_of(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase()
}

/// Distinct running applications, de-duplicated by executable path.
/// Processes we cannot inspect are skipped rather than shown as blanks.
pub fn list_running_apps() -> Vec<RunningApp> {
    let mut seen: HashMap<String, RunningApp> = HashMap::new();
    for pid in enum_pids() {
        let path = process_path(pid);
        if path.is_empty() {
            continue;
        }
        let name = file_name_of(&path);
        if name.is_empty() {
            continue;
        }
        seen.entry(path.to_ascii_lowercase())
            .or_insert(RunningApp { pid, name, path });
    }
    let mut out: Vec<RunningApp> = seen.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Does a running process at `path` match an app rule?
///
/// A rule may be a full path ("C:\...\chrome.exe") or a bare name ("chrome.exe").
/// Bare names match any location, which is what users expect when they type one.
pub fn app_matches(rule_value: &str, proc_path: &str) -> bool {
    let rule = rule_value.trim().to_ascii_lowercase().replace('/', "\\");
    if rule.is_empty() || proc_path.is_empty() {
        return false;
    }
    let proc_lower = proc_path.to_ascii_lowercase().replace('/', "\\");

    if rule.contains('\\') {
        proc_lower == rule
    } else {
        file_name_of(&proc_lower) == rule
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_name_matches_any_path() {
        assert!(app_matches("chrome.exe", r"C:\Program Files\Chrome\chrome.exe"));
        assert!(app_matches("CHROME.EXE", r"D:\other\chrome.exe"));
        assert!(!app_matches("chrome.exe", r"C:\x\firefox.exe"));
    }

    #[test]
    fn full_path_is_exact() {
        assert!(app_matches(
            r"C:\Games\game.exe",
            r"c:\games\GAME.exe"
        ));
        assert!(!app_matches(r"C:\Games\game.exe", r"C:\Other\game.exe"));
    }

    #[test]
    fn slashes_normalized() {
        assert!(app_matches("C:/Games/game.exe", r"C:\Games\game.exe"));
    }

    #[test]
    fn empty_never_matches() {
        assert!(!app_matches("", r"C:\x\a.exe"));
        assert!(!app_matches("a.exe", ""));
    }

    #[test]
    fn file_name_extraction() {
        assert_eq!(file_name_of(r"C:\a\b\c.EXE"), "c.exe");
        assert_eq!(file_name_of("plain.exe"), "plain.exe");
    }
}
