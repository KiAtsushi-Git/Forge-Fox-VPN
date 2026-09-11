/// Per-flow routing rules for "union mode" (Proxy split tunneling with both
/// apps and sites selected).
///
/// In union mode the TUN routes 0.0.0.0/0 with no app filter, so every
/// app's traffic enters the TUN. For each new flow (TCP SYN or first UDP
/// packet) the core decides:
///   - destination inside a selected site range → tunnel via SSH;
///   - owner (uid) of the connection is a selected app → tunnel via SSH;
///   - otherwise → bypass: re-originate through a protected socket outside
///     the VPN (UDP relay / userspace TCP).
///
/// This module is pure logic (no I/O) so it is unit-testable on a dev host,
/// like `dns`. The tokio glue lives in `ssh_vpn`.
use std::collections::{HashMap, VecDeque};

/// IP protocol numbers we can route per-flow.
pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;

/// Forward-flow identity: (proto, src ip, src port, dst ip, dst port),
/// addresses in network byte order.
pub type FlowKey = (u8, u32, u16, u32, u16);

/// What to do with packets of a flow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// Forward through the SSH tunnel (raw IP packets, as before).
    Tunnel,
    /// Re-originate outside the VPN through a protected socket.
    Bypass,
}

/// Forwarding decision for one outbound TUN packet, made by `UnionEngine`.
#[derive(Debug, PartialEq, Eq)]
pub enum FlowAction {
    /// A decided flow (existing or just created): follow it.
    Route(Route),
    /// No flow exists and the packet cannot start one (e.g. a mid-connection
    /// TCP packet after the flow was reaped, or a non-first IP fragment).
    /// Forwarding it would leak; dropping is safe — the endpoint retransmits
    /// or times out.
    Drop,
}

/// Sorted, merged IPv4 ranges (network byte order u32 pairs).
#[derive(Clone, Debug, Default)]
pub struct RangeSet {
    ranges: Vec<(u32, u32)>,
}

impl RangeSet {
    /// Build from arbitrary (start, end) pairs; empty ranges dropped,
    /// input sorted and overlapping/adjacent ranges merged.
    pub fn from_pairs(mut pairs: Vec<(u32, u32)>) -> Self {
        pairs.retain(|(s, e)| s <= e);
        pairs.sort_unstable();
        let mut merged: Vec<(u32, u32)> = Vec::with_capacity(pairs.len());
        for (s, e) in pairs {
            match merged.last_mut() {
                Some((_, last_e)) if s <= last_e.wrapping_add(1) => {
                    if e > *last_e {
                        *last_e = e;
                    }
                }
                _ => merged.push((s, e)),
            }
        }
        Self { ranges: merged }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    /// True if `ip` falls inside any range.
    pub fn contains(&self, ip: u32) -> bool {
        let idx = self.ranges.partition_point(|&(_, e)| e < ip);
        matches!(self.ranges.get(idx), Some(&(s, _)) if s <= ip)
    }
}

/// Forward-flow table with FIFO eviction: the routing decision for a flow is
/// cached from its first packet so later packets (ACKs, data, window
/// updates) follow the same path.
pub struct FlowTable {
    map: HashMap<FlowKey, Route>,
    order: VecDeque<FlowKey>,
    cap: usize,
}

impl FlowTable {
    pub fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub fn route_for(&self, key: &FlowKey) -> Option<Route> {
        self.map.get(key).copied()
    }

    /// Remember the decision for a flow (re-inserting refreshes FIFO order).
    pub fn set(&mut self, key: FlowKey, route: Route) {
        if !self.map.contains_key(&key) {
            if self.map.len() >= self.cap {
                // Evict the oldest entry; bypass conn tasks remove their own
                // keys, so entries here are usually cheap re-decisions.
                while let Some(old) = self.order.pop_front() {
                    if self.map.remove(&old).is_some() {
                        break;
                    }
                }
            }
            self.order.push_back(key);
        }
        self.map.insert(key, route);
    }

    /// Drop a flow (called by bypass tasks when the connection ends).
    pub fn remove(&mut self, key: &FlowKey) {
        if self.map.remove(key).is_some() {
            // Leave the stale key in `order`; the eviction loop skips
            // missing entries, and re-set after removal re-appends.
            self.order.retain(|k| k != key);
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// Parse the flow key out of an IPv4 packet.
/// Returns `None` for anything without a TCP/UDP pseudo-header:
/// non-IPv4, truncated, or non-first IP fragments.
pub fn flow_key_of(pkt: &[u8]) -> Option<FlowKey> {
    if pkt.len() < 20 || pkt[0] >> 4 != 4 {
        return None;
    }
    let ihl = ((pkt[0] & 0x0F) as usize) * 4;
    if ihl < 20 || pkt.len() < ihl + 8 {
        return None;
    }
    // Fragmented: only the first fragment carries the TCP/UDP header.
    // Non-first fragments cannot be attributed to a flow.
    let frag = u16::from_be_bytes([pkt[6], pkt[7]]);
    if frag & 0x1FFF != 0 {
        return None;
    }
    let proto = pkt[9];
    if proto != PROTO_TCP && proto != PROTO_UDP {
        return None;
    }
    let src = u32::from_be_bytes([pkt[12], pkt[13], pkt[14], pkt[15]]);
    let dst = u32::from_be_bytes([pkt[16], pkt[17], pkt[18], pkt[19]]);
    let sport = u16::from_be_bytes([pkt[ihl], pkt[ihl + 1]]);
    let dport = u16::from_be_bytes([pkt[ihl + 2], pkt[ihl + 3]]);
    Some((proto, src, sport, dst, dport))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(a: u8, b: u8, c: u8, d: u8) -> u32 {
        u32::from_be_bytes([a, b, c, d])
    }

    #[test]
    fn range_set_contains_and_merges() {
        let rs = RangeSet::from_pairs(vec![
            (ip(1, 2, 3, 4), ip(1, 2, 3, 4)),
            (ip(1, 2, 3, 10), ip(1, 2, 3, 20)),
            (ip(1, 2, 3, 19), ip(1, 2, 4, 0)), // overlaps + extends previous
            (ip(8, 8, 8, 8), ip(8, 8, 8, 9)),
            (ip(9, 9, 9, 9), ip(9, 9, 9, 1)), // reversed → dropped
        ]);
        assert_eq!(rs.len(), 3);
        assert!(rs.contains(ip(1, 2, 3, 4)));
        assert!(rs.contains(ip(1, 2, 3, 15)));
        assert!(rs.contains(ip(1, 2, 4, 0)));
        assert!(!rs.contains(ip(1, 2, 3, 5)));  // gap between the singles
        assert!(!rs.contains(ip(1, 2, 3, 9)));  // below the merged range
        assert!(!rs.contains(ip(1, 2, 4, 1)));  // above the merged range
        assert!(!rs.contains(ip(8, 8, 8, 10)));
        assert!(!rs.contains(ip(9, 9, 9, 9)));
    }

    #[test]
    fn range_set_boundaries() {
        let rs = RangeSet::from_pairs(vec![(0, u32::MAX)]);
        assert!(rs.contains(0));
        assert!(rs.contains(u32::MAX));
        assert!(rs.contains(ip(198, 18, 0, 2)));
        let none = RangeSet::from_pairs(vec![]);
        assert!(none.is_empty());
        assert!(!none.contains(0));
    }

    #[test]
    fn flow_table_follows_and_evicts() {
        let mut t = FlowTable::new(2);
        let k1 = (PROTO_TCP, 1, 1, 1, 1);
        let k2 = (PROTO_TCP, 2, 2, 2, 2);
        let k3 = (PROTO_TCP, 3, 3, 3, 3);
        t.set(k1, Route::Tunnel);
        t.set(k2, Route::Bypass);
        assert_eq!(t.route_for(&k1), Some(Route::Tunnel));
        assert_eq!(t.route_for(&k2), Some(Route::Bypass));
        t.set(k3, Route::Tunnel); // evicts k1 (oldest)
        assert_eq!(t.route_for(&k1), None);
        assert_eq!(t.route_for(&k3), Some(Route::Tunnel));
        t.remove(&k3);
        assert_eq!(t.route_for(&k3), None);
        assert_eq!(t.len(), 1);
        // re-insert after remove works
        t.set(k3, Route::Bypass);
        assert_eq!(t.route_for(&k3), Some(Route::Bypass));
    }

    #[test]
    fn flow_key_parses_tcp_and_udp() {
        // IPv4 + TCP SYN, 20-byte IP header, 20-byte TCP header
        let mut pkt = vec![0u8; 40];
        pkt[0] = 0x45;
        pkt[9] = PROTO_TCP;
        pkt[12..16].copy_from_slice(&ip(10, 1, 2, 3).to_be_bytes());
        pkt[16..20].copy_from_slice(&ip(8, 8, 8, 8).to_be_bytes());
        pkt[20..22].copy_from_slice(&44321u16.to_be_bytes());
        pkt[22..24].copy_from_slice(&443u16.to_be_bytes());
        let key = flow_key_of(&pkt).unwrap();
        assert_eq!(key, (PROTO_TCP, ip(10, 1, 2, 3), 44321, ip(8, 8, 8, 8), 443));

        // UDP
        let mut udp = vec![0u8; 28];
        udp[0] = 0x45;
        udp[9] = PROTO_UDP;
        let key = flow_key_of(&udp).unwrap();
        assert_eq!(key.0, PROTO_UDP);
    }

    #[test]
    fn flow_key_rejects_fragments_and_garbage() {
        // Non-first fragment (offset 5)
        let mut pkt = vec![0x45u8, 0, 0, 0, 0, 0, 0x00, 0x05, 64, PROTO_TCP, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(flow_key_of(&pkt), None);
        // First fragment (MF set, offset 0) is fine
        pkt[6] = 0x20; // MF flag
        pkt[7] = 0x00;
        let mut tcp = vec![0u8; 40];
        tcp[..20].copy_from_slice(&pkt);
        assert!(flow_key_of(&tcp).is_some());
        // Too short
        assert_eq!(flow_key_of(&[0x45, 0]), None);
        // IPv6
        let mut v6 = vec![0u8; 48];
        v6[0] = 0x60;
        assert_eq!(flow_key_of(&v6), None);
    }
}
