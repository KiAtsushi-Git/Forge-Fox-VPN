/// Windows route-table management via `route` / `netsh` CLI tools.
/// All operations are best-effort (log errors, don't panic).
///
/// This module only holds *primitives*. The stateful split-tunnel logic that
/// decides which prefixes to install lives in `vpn::split`.
use anyhow::{bail, Result};
use std::net::{IpAddr, Ipv4Addr};
use std::process::Command;

pub const TUN_IFACE: &str = "ForgeFoxVPN";

// ── Helpers ───────────────────────────────────────────────────────────────────

fn run(args: &[&str]) -> Result<String> {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let out = cmd.output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        bail!("Command {:?} failed: {}{}", args, stdout, stderr);
    }
    Ok(stdout)
}

fn run_silent(args: &[&str]) {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    match cmd.output() {
        Ok(out) => {
            if !out.status.success() {
                crate::vpn::ssh_vpn::vpn_log_ext(format!(
                    "Route err: {:?} -> {}",
                    args,
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
        }
        Err(e) => {
            crate::vpn::ssh_vpn::vpn_log_ext(format!("Route exec err: {:?} -> {}", args, e));
        }
    }
}

// ── Default gateway discovery ─────────────────────────────────────────────────

/// Returns the current default IPv4 gateway (lowest-metric one in the table).
pub fn get_default_gateway() -> Result<String> {
    let out = run(&["route", "print", "0.0.0.0"])?;
    let mut best_gw = None;
    let mut best_metric = u32::MAX;

    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Expected format: "0.0.0.0  0.0.0.0  <gateway>  <iface>  <metric>"
        if parts.len() >= 5 && parts[0] == "0.0.0.0" && parts[1] == "0.0.0.0" {
            let gw = parts[2];
            // Skip our own TUN gateway if a stale default route is still present.
            if gw.parse::<IpAddr>().is_ok() && gw != "0.0.0.0" {
                if let Ok(metric) = parts[4].parse::<u32>() {
                    if metric < best_metric {
                        best_metric = metric;
                        best_gw = Some(gw.to_string());
                    }
                }
            }
        }
    }

    if let Some(gw) = best_gw {
        return Ok(gw);
    }
    bail!("Default gateway not found in route table")
}

/// Returns the metric of the current default route (used to set TUN lower metric).
pub fn get_default_metric() -> u32 {
    if let Ok(out) = run(&["route", "print", "0.0.0.0"]) {
        for line in out.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 && parts[0] == "0.0.0.0" && parts[1] == "0.0.0.0" {
                if let Ok(m) = parts[4].parse::<u32>() {
                    return m;
                }
            }
        }
    }
    100
}

// ── CIDR parsing ──────────────────────────────────────────────────────────────

/// Parse "1.2.3.4" or "10.0.0.0/8" into (network address, prefix length),
/// masking off any host bits the user typed (10.1.2.3/8 → 10.0.0.0/8).
pub fn parse_prefix(cidr: &str) -> Option<(Ipv4Addr, u8)> {
    let cidr = cidr.trim();
    let (addr_s, prefix) = match cidr.split_once('/') {
        Some((a, p)) => (a, p.parse::<u8>().ok()?),
        None => (cidr, 32u8),
    };
    if prefix > 32 {
        return None;
    }
    let addr: Ipv4Addr = addr_s.trim().parse().ok()?;
    let mask: u32 = if prefix == 0 { 0 } else { !0u32 << (32 - prefix) };
    Some((Ipv4Addr::from(u32::from(addr) & mask), prefix))
}

/// Canonical "network/prefix" string — the key used for route bookkeeping.
pub fn canonical_prefix(cidr: &str) -> Option<String> {
    let (net, prefix) = parse_prefix(cidr)?;
    Some(format!("{net}/{prefix}"))
}

fn prefix_to_net_mask(prefix_str: &str) -> Option<(String, String)> {
    let (net, prefix) = parse_prefix(prefix_str)?;
    let mask: u32 = if prefix == 0 { 0 } else { !0u32 << (32 - prefix) };
    Some((net.to_string(), Ipv4Addr::from(mask).to_string()))
}

// ── Route add / delete ────────────────────────────────────────────────────────

/// Add a host route so the SSH server IP bypasses the VPN (prevents routing loop).
pub fn add_host_route(ssh_server_ip: &str, gateway: &str) {
    run_silent(&[
        "route", "add", ssh_server_ip, "mask", "255.255.255.255", gateway, "metric", "5",
    ]);
}

/// Remove previously added host route for the SSH server.
pub fn del_host_route(ssh_server_ip: &str) {
    run_silent(&["route", "delete", ssh_server_ip, "mask", "255.255.255.255"]);
}

/// Add a default route via the TUN adapter using the 0.0.0.0/1 + 128.0.0.0/1 trick.
/// Only used in Bypass mode — Proxy mode deliberately leaves the default route alone.
pub fn add_tun_default_route(tun_iface: &str, tun_gw: &str) {
    run_silent(&["route", "delete", "0.0.0.0", "mask", "128.0.0.0"]);
    run_silent(&["route", "delete", "128.0.0.0", "mask", "128.0.0.0"]);
    // netsh pins the route to the interface *by name*, which `route add` cannot do.
    run_silent(&[
        "netsh", "interface", "ipv4", "add", "route",
        "0.0.0.0/1", tun_iface, tun_gw, "metric=1", "store=active",
    ]);
    run_silent(&[
        "netsh", "interface", "ipv4", "add", "route",
        "128.0.0.0/1", tun_iface, tun_gw, "metric=1", "store=active",
    ]);
}

/// Remove default route via TUN.
pub fn del_tun_default_route(_tun_gw: &str) {
    run_silent(&["route", "delete", "0.0.0.0", "mask", "128.0.0.0"]);
    run_silent(&["route", "delete", "128.0.0.0", "mask", "128.0.0.0"]);
}

/// Route `prefix` around the tunnel, through the real physical gateway.
pub fn add_route_via_gateway(prefix: &str, gateway: &str, metric: u32) -> bool {
    let Some((net, mask)) = prefix_to_net_mask(prefix) else {
        crate::vpn::ssh_vpn::vpn_log_ext(format!("Bad prefix skipped: {prefix}"));
        return false;
    };
    run_silent(&[
        "route", "add", &net, "mask", &mask, gateway, "metric", &metric.to_string(),
    ]);
    true
}

/// Route `prefix` *into* the tunnel (used by Proxy mode).
pub fn add_route_via_tun(prefix: &str, tun_iface: &str, tun_gw: &str, metric: u32) -> bool {
    let Some(canon) = canonical_prefix(prefix) else {
        crate::vpn::ssh_vpn::vpn_log_ext(format!("Bad prefix skipped: {prefix}"));
        return false;
    };
    run_silent(&[
        "netsh", "interface", "ipv4", "add", "route",
        &canon, tun_iface, tun_gw,
        &format!("metric={metric}"),
        "store=active",
    ]);
    true
}

/// Delete a previously installed split route (either direction).
pub fn del_route(prefix: &str) {
    let Some((net, mask)) = prefix_to_net_mask(prefix) else { return };
    run_silent(&["route", "delete", &net, "mask", &mask]);
}

// ── DNS helpers ───────────────────────────────────────────────────────────────

/// Resolve a domain name to IPv4 addresses synchronously (best effort).
pub fn resolve_domain(domain: &str) -> Vec<Ipv4Addr> {
    use std::net::ToSocketAddrs;
    format!("{domain}:80")
        .to_socket_addrs()
        .map(|iter| {
            iter.filter_map(|a| match a.ip() {
                IpAddr::V4(v4) => Some(v4),
                IpAddr::V6(_) => None,
            })
            .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_host_bits() {
        assert_eq!(canonical_prefix("10.1.2.3/8").as_deref(), Some("10.0.0.0/8"));
        assert_eq!(
            canonical_prefix("192.168.1.0/24").as_deref(),
            Some("192.168.1.0/24")
        );
        assert_eq!(canonical_prefix("1.2.3.4").as_deref(), Some("1.2.3.4/32"));
        assert_eq!(canonical_prefix("0.0.0.0/0").as_deref(), Some("0.0.0.0/0"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(canonical_prefix("not-an-ip").is_none());
        assert!(canonical_prefix("10.0.0.0/33").is_none());
        assert!(canonical_prefix("").is_none());
    }

    #[test]
    fn net_mask_conversion() {
        assert_eq!(
            prefix_to_net_mask("10.0.0.0/8"),
            Some(("10.0.0.0".into(), "255.0.0.0".into()))
        );
        assert_eq!(
            prefix_to_net_mask("1.2.3.4"),
            Some(("1.2.3.4".into(), "255.255.255.255".into()))
        );
    }
}
