/// Split-tunnel engine.
///
/// Owns every route this app installs and keeps the OS route table in sync with
/// the current ruleset while the VPN is up. Two modes:
///
///   * **Bypass** — default route points into the tunnel; matched traffic is
///     pulled back out through the physical gateway.
///   * **Proxy** — default route is left alone; only matched traffic is pushed
///     into the tunnel.
///
/// Both modes therefore reduce to "install a route per matched prefix"; only
/// the next hop and the base route differ. That symmetry is what makes Proxy
/// mode work at all — the previous implementation installed a tunnel default
/// route unconditionally, so "only the list via VPN" silently sent everything
/// through the VPN.
use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;

use crate::config::{RuleType, SplitMode, SplitRule};
use crate::vpn::{dns, process, routing};

/// Metric for rule routes. Below the tunnel's default (1) so /32s always win.
const RULE_METRIC: u32 = 3;

/// Why a prefix is currently routed. A prefix can be justified by several rules
/// at once (two apps hitting the same CDN), so removal is refcounted by source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Source {
    /// Static rule (ip / cidr), keyed by rule id
    Static(String),
    /// Learned from DNS for a domain/zone rule
    Dns { rule_id: String, host: String },
    /// Learned from the TCP table for an app rule
    App { rule_id: String, pid: u32 },
    /// Resolver pulled into the tunnel so Proxy mode can observe DNS answers
    Resolver,
}

pub struct SplitEngine {
    mode: SplitMode,
    enabled: bool,
    rules: Vec<SplitRule>,

    /// Real physical gateway (next hop for Bypass exclusions)
    gateway: String,
    /// Tunnel gateway (next hop for Proxy inclusions)
    tun_gw: String,
    tun_iface: String,
    /// Configured resolvers, needed to observe DNS in Proxy mode
    dns_servers: Vec<String>,

    /// prefix → set of reasons it is installed
    installed: HashMap<String, HashSet<Source>>,

    /// Cache so we don't re-check the same PID on every poll tick.
    /// pid → matching rule ids (empty vec = known non-match)
    pid_cache: HashMap<u32, Vec<String>>,

    /// True once base routes are in place.
    active: bool,
}

impl SplitEngine {
    pub fn new(
        mode: SplitMode,
        enabled: bool,
        rules: Vec<SplitRule>,
        gateway: String,
        tun_gw: String,
        dns_servers: Vec<String>,
    ) -> Self {
        Self {
            mode,
            enabled,
            rules,
            gateway,
            tun_gw,
            tun_iface: routing::TUN_IFACE.to_string(),
            dns_servers,
            installed: HashMap::new(),
            pid_cache: HashMap::new(),
            active: false,
        }
    }

    pub fn mode(&self) -> SplitMode {
        self.mode
    }

    /// Whether split rules are actually in force. With split disabled we behave
    /// like a plain full-tunnel VPN.
    fn rules_active(&self) -> bool {
        self.enabled && self.effective_rules().next().is_some()
    }

    fn effective_rules(&self) -> impl Iterator<Item = &SplitRule> {
        self.rules.iter().filter(|r| r.enabled)
    }

    // ── Base routes ───────────────────────────────────────────────────────────

    /// Install the mode's base route. Call once after the TUN is up.
    pub fn activate(&mut self) {
        if self.active {
            return;
        }
        self.active = true;

        let proxy_mode = self.enabled && self.mode == SplitMode::Proxy;

        if proxy_mode {
            // Deliberately no tunnel default route: unmatched traffic must keep
            // using the physical link.
            log(
                "Split: режим PROXY — по умолчанию трафик идёт напрямую, \
                 в туннель уходит только список.",
            );
        } else {
            routing::add_tun_default_route(&self.tun_iface, &self.tun_gw);
            if self.enabled {
                log("Split: режим BYPASS — весь трафик через VPN, кроме списка.");
            }
        }

        self.sync_static_rules();
        self.sync_resolver_routes();
    }

    fn has_domain_rules(&self) -> bool {
        self.effective_rules()
            .any(|r| matches!(r.rule_type, RuleType::Domain | RuleType::DomainZone))
    }

    /// In Proxy mode nothing but the rule list enters the tunnel, so DNS answers
    /// never cross it and domain/zone rules would never learn anything. Pull the
    /// configured resolvers inside so their replies are observable. This also
    /// avoids leaking which proxied hosts are being looked up.
    fn sync_resolver_routes(&mut self) {
        let want = self.enabled && self.mode == SplitMode::Proxy && self.has_domain_rules();

        if !want {
            self.uninstall_source(|s| matches!(s, Source::Resolver));
            return;
        }

        let servers = self.dns_servers.clone();
        for s in servers {
            if s.trim().is_empty() {
                continue;
            }
            self.install(s.trim(), Source::Resolver);
        }
        log("Split: DNS-серверы направлены в туннель для отслеживания доменных зон.");
    }

    /// Tear down every route this engine installed.
    pub fn deactivate(&mut self) {
        let proxy_mode = self.enabled && self.mode == SplitMode::Proxy;
        if !proxy_mode {
            routing::del_tun_default_route(&self.tun_gw);
        }

        // Delete by recorded key, never by re-resolving: a domain that changed
        // address since install would otherwise leak a route.
        let prefixes: Vec<String> = self.installed.keys().cloned().collect();
        for p in prefixes {
            routing::del_route(&p);
        }
        self.installed.clear();
        self.pid_cache.clear();
        self.active = false;
        log("Split: маршруты исключений сняты.");
    }

    // ── Route bookkeeping ─────────────────────────────────────────────────────

    fn next_hop_is_tunnel(&self) -> bool {
        self.mode == SplitMode::Proxy
    }

    fn install(&mut self, prefix_input: &str, source: Source) {
        let Some(prefix) = routing::canonical_prefix(prefix_input) else {
            log(format!("Split: пропущено некорректное значение «{prefix_input}»"));
            return;
        };

        // Never hijack the tunnel endpoints or loopback.
        if let Some((net, plen)) = routing::parse_prefix(&prefix) {
            if net.is_loopback() || (net.is_unspecified() && plen == 0) {
                log(format!("Split: значение «{prefix}» проигнорировано (небезопасный префикс)"));
                return;
            }
        }

        let entry = self.installed.entry(prefix.clone()).or_default();
        let first = entry.is_empty();
        entry.insert(source);

        if first {
            let ok = if self.next_hop_is_tunnel() {
                routing::add_route_via_tun(&prefix, &self.tun_iface, &self.tun_gw, RULE_METRIC)
            } else {
                routing::add_route_via_gateway(&prefix, &self.gateway, RULE_METRIC)
            };
            if !ok {
                self.installed.remove(&prefix);
            }
        }
    }

    fn uninstall_source(&mut self, pred: impl Fn(&Source) -> bool) {
        let mut empty: Vec<String> = Vec::new();
        for (prefix, sources) in self.installed.iter_mut() {
            sources.retain(|s| !pred(s));
            if sources.is_empty() {
                empty.push(prefix.clone());
            }
        }
        for prefix in empty {
            routing::del_route(&prefix);
            self.installed.remove(&prefix);
        }
    }

    // ── Static rules (ip / cidr) ──────────────────────────────────────────────

    fn sync_static_rules(&mut self) {
        if !self.rules_active() {
            return;
        }

        // Collect first: `install` borrows self mutably.
        let wanted: Vec<(String, String)> = self
            .effective_rules()
            .filter(|r| matches!(r.rule_type, RuleType::Ip | RuleType::Cidr))
            .map(|r| (r.id.clone(), r.normalized_value()))
            .collect();

        for (id, value) in wanted {
            self.install(&value, Source::Static(id));
        }

        // Seed domain rules with a one-shot resolve so common cases work
        // immediately; the DNS observer keeps them fresh afterwards.
        let domains: Vec<(String, String)> = self
            .effective_rules()
            .filter(|r| matches!(r.rule_type, RuleType::Domain | RuleType::DomainZone))
            .map(|r| (r.id.clone(), r.normalized_value()))
            .collect();

        for (id, host) in domains {
            for ip in routing::resolve_domain(&host) {
                self.install(
                    &ip.to_string(),
                    Source::Dns { rule_id: id.clone(), host: host.clone() },
                );
            }
        }
    }

    // ── Live rule updates ─────────────────────────────────────────────────────

    /// Replace the ruleset while connected and reconcile routes in place.
    /// Switching mode requires a full rebuild because the next hop changes.
    pub fn update_rules(&mut self, enabled: bool, mode: SplitMode, rules: Vec<SplitRule>) {
        let structural = mode != self.mode || enabled != self.enabled;

        if structural {
            let was_active = self.active;
            if was_active {
                self.deactivate();
            }
            self.mode = mode;
            self.enabled = enabled;
            self.rules = rules;
            if was_active {
                self.activate();
            }
            return;
        }

        // Same mode: drop routes for rules that vanished or were switched off,
        // then install whatever is newly present.
        let live: HashSet<String> = rules
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.id.clone())
            .collect();

        self.uninstall_source(|s| {
            let id = match s {
                Source::Static(id) => id,
                Source::Dns { rule_id, .. } => rule_id,
                Source::App { rule_id, .. } => rule_id,
                Source::Resolver => return false,
            };
            !live.contains(id)
        });

        self.rules = rules;
        self.pid_cache.clear(); // rule values may have changed
        if self.active {
            self.sync_static_rules();
            self.sync_resolver_routes();
        }
    }

    // ── DNS observation ───────────────────────────────────────────────────────

    /// Feed a packet seen on the tunnel. DNS answers matching a domain or zone
    /// rule install routes for the addresses they carry.
    pub fn observe_packet(&mut self, pkt: &[u8]) {
        if !self.rules_active() {
            return;
        }
        let Some(answer) = dns::parse_ipv4_dns_packet(pkt) else { return };
        self.observe_dns(&answer);
    }

    fn observe_dns(&mut self, answer: &dns::DnsAnswer) {
        let matches: Vec<(String, String)> = self
            .effective_rules()
            .filter_map(|r| {
                let value = r.normalized_value();
                let hit = match r.rule_type {
                    RuleType::Domain => answer
                        .names
                        .iter()
                        .any(|n| n.trim_end_matches('.').eq_ignore_ascii_case(&value)),
                    RuleType::DomainZone => {
                        answer.names.iter().any(|n| dns::in_zone(n, &value))
                    }
                    _ => false,
                };
                hit.then(|| (r.id.clone(), value))
            })
            .collect();

        if matches.is_empty() {
            return;
        }

        let ips: Vec<Ipv4Addr> = answer.ips.clone();
        for (rule_id, host) in matches {
            for ip in &ips {
                let before = self.installed.len();
                self.install(
                    &ip.to_string(),
                    Source::Dns { rule_id: rule_id.clone(), host: host.clone() },
                );
                if self.installed.len() != before {
                    log(format!(
                        "Split: {} → {} ({})",
                        answer.query,
                        ip,
                        if self.next_hop_is_tunnel() { "в туннель" } else { "мимо туннеля" }
                    ));
                }
            }
        }
    }

    // ── App rules ─────────────────────────────────────────────────────────────

    /// Poll the TCP table and route remote endpoints of listed applications.
    /// Cheap enough for a ~1s tick: one syscall plus a lookup per new PID.
    pub fn poll_apps(&mut self) {
        if !self.rules_active() {
            return;
        }
        let app_rules: Vec<(String, String)> = self
            .effective_rules()
            .filter(|r| r.rule_type == RuleType::App)
            .map(|r| (r.id.clone(), r.normalized_value()))
            .collect();

        if app_rules.is_empty() {
            return;
        }

        let conns = process::list_tcp_connections();
        let mut live_pids: HashSet<u32> = HashSet::new();

        for conn in conns {
            live_pids.insert(conn.pid);

            let rule_ids = match self.pid_cache.get(&conn.pid) {
                Some(ids) => ids.clone(),
                None => {
                    let path = process::process_path(conn.pid);
                    let ids: Vec<String> = if path.is_empty() {
                        Vec::new()
                    } else {
                        app_rules
                            .iter()
                            .filter(|(_, value)| process::app_matches(value, &path))
                            .map(|(id, _)| id.clone())
                            .collect()
                    };
                    self.pid_cache.insert(conn.pid, ids.clone());
                    ids
                }
            };

            for rule_id in rule_ids {
                self.install(
                    &conn.remote.to_string(),
                    Source::App { rule_id, pid: conn.pid },
                );
            }
        }

        // Forget dead processes so a recycled PID is re-evaluated, and drop the
        // routes they justified.
        self.pid_cache.retain(|pid, _| live_pids.contains(pid));
        self.uninstall_source(|s| match s {
            Source::App { pid, .. } => !live_pids.contains(pid),
            _ => false,
        });
    }

    /// Number of routes currently installed (surfaced in the UI).
    pub fn installed_count(&self) -> usize {
        self.installed.len()
    }
}

fn log(msg: impl Into<String>) {
    crate::vpn::ssh_vpn::vpn_log_ext(msg.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(t: RuleType, v: &str) -> SplitRule {
        SplitRule::new(t, v.to_string())
    }

    fn engine(mode: SplitMode, rules: Vec<SplitRule>) -> SplitEngine {
        SplitEngine::new(
            mode,
            true,
            rules,
            "192.168.1.1".into(),
            "10.42.7.1".into(),
            vec!["8.8.8.8".into()],
        )
    }

    #[test]
    fn next_hop_follows_mode() {
        assert!(!engine(SplitMode::Bypass, vec![]).next_hop_is_tunnel());
        assert!(engine(SplitMode::Proxy, vec![]).next_hop_is_tunnel());
    }

    #[test]
    fn rules_inactive_when_disabled_or_empty() {
        let e = SplitEngine::new(
            SplitMode::Bypass,
            false,
            vec![rule(RuleType::Ip, "1.2.3.4")],
            "192.168.1.1".into(),
            "10.42.7.1".into(),
            vec![],
        );
        assert!(!e.rules_active(), "disabled split must not apply rules");

        assert!(!engine(SplitMode::Bypass, vec![]).rules_active());

        let mut off = rule(RuleType::Ip, "1.2.3.4");
        off.enabled = false;
        assert!(!engine(SplitMode::Bypass, vec![off]).rules_active());

        assert!(engine(SplitMode::Bypass, vec![rule(RuleType::Ip, "1.2.3.4")]).rules_active());
    }

    #[test]
    fn refcounting_keeps_shared_prefix_until_last_source() {
        let mut e = engine(SplitMode::Bypass, vec![]);
        // Bypass installs via `route.exe`; in tests the command just fails and
        // the entry is rolled back, so drive bookkeeping directly.
        e.installed
            .entry("5.5.5.5/32".into())
            .or_default()
            .insert(Source::Static("a".into()));
        e.installed
            .get_mut("5.5.5.5/32")
            .unwrap()
            .insert(Source::Static("b".into()));

        e.uninstall_source(|s| matches!(s, Source::Static(id) if id == "a"));
        assert!(e.installed.contains_key("5.5.5.5/32"), "still held by rule b");

        e.uninstall_source(|s| matches!(s, Source::Static(id) if id == "b"));
        assert!(!e.installed.contains_key("5.5.5.5/32"), "last source removed");
    }

    #[test]
    fn zone_rule_matches_subdomain_answer() {
        let e = engine(SplitMode::Proxy, vec![rule(RuleType::DomainZone, "example.com")]);
        let answer = dns::DnsAnswer {
            query: "cdn.example.com".into(),
            names: vec!["cdn.example.com".into()],
            ips: vec![Ipv4Addr::new(9, 9, 9, 9)],
        };
        let hit = e.effective_rules().any(|r| {
            r.rule_type == RuleType::DomainZone
                && answer.names.iter().any(|n| dns::in_zone(n, &r.normalized_value()))
        });
        assert!(hit);
    }

    #[test]
    fn exact_domain_rule_ignores_subdomain() {
        let e = engine(SplitMode::Proxy, vec![rule(RuleType::Domain, "example.com")]);
        let names = vec!["cdn.example.com".to_string()];
        let hit = e.effective_rules().any(|r| {
            names.iter().any(|n| n.eq_ignore_ascii_case(&r.normalized_value()))
        });
        assert!(!hit, "exact domain must not match a subdomain");
    }

    #[test]
    fn resolver_routes_only_wanted_for_proxy_with_domain_rules() {
        // Proxy + domain rule → resolvers must be pulled into the tunnel.
        let e = engine(SplitMode::Proxy, vec![rule(RuleType::DomainZone, "example.com")]);
        assert!(e.has_domain_rules());
        assert!(e.enabled && e.mode == SplitMode::Proxy);

        // Bypass never needs it: DNS already crosses the tunnel by default.
        let e = engine(SplitMode::Bypass, vec![rule(RuleType::DomainZone, "example.com")]);
        assert!(!(e.enabled && e.mode == SplitMode::Proxy && e.has_domain_rules()));

        // Proxy with only IP rules has nothing to learn.
        let e = engine(SplitMode::Proxy, vec![rule(RuleType::Ip, "1.2.3.4")]);
        assert!(!e.has_domain_rules());
    }

    #[test]
    fn resolver_source_survives_rule_removal() {
        let mut e = engine(SplitMode::Proxy, vec![]);
        e.installed
            .entry("8.8.8.8/32".into())
            .or_default()
            .insert(Source::Resolver);

        // Removing every rule must not strip the resolver route.
        e.uninstall_source(|s| !matches!(s, Source::Resolver));
        assert!(e.installed.contains_key("8.8.8.8/32"));
    }
}
