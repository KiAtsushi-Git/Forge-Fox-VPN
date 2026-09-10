/// Fake-IP DNS interception support.
///
/// In Proxy mode with wildcard zones (*.ru) the VpnService routes only the
/// fake-IP pool 198.18.0.0/15 into the TUN and points the system resolver at
/// a virtual DNS server (198.18.0.2) served by the native core. This module
/// provides the pieces:
///  - parse the client's DNS queries and build answers,
///  - talk to a real upstream resolver through a protected socket,
///  - keep the bidirectional fake↔real address pool (FIFO eviction).
use std::collections::{HashMap, VecDeque};

/// First fake IP handed out by the pool. 198.18.0.0/15 minus the low
/// addresses (network / DNS server / broadcast-ish) — the pool grows upward.
const POOL_FIRST: u32 = 0xC612_000A; // 198.18.0.10
const POOL_LAST: u32 = 0xC613_FFFE; // 198.19.255.254

/// True when `ip` (network byte order) sits inside the fake-IP range.
pub fn is_fake_ip(ip: u32) -> bool {
    (ip & 0xFFFE_0000) == 0xC612_0000 // 198.18.0.0/15
}

/// A single-question DNS query (the only kind Android's resolver sends).
pub struct DnsQuery {
    pub id: u16,
    pub flags: u16,
    pub name: String,
    pub qtype: u16,
}

/// Read a (possibly compressed) DNS name starting at `pos`.
/// Returns the decoded name and the offset just past the name in the
/// original buffer (compression pointers do not advance the outer cursor).
fn read_name(buf: &[u8], start: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = start;
    let mut end = start;
    let mut jumped = false;
    let mut hops = 0usize;

    loop {
        let len = *buf.get(pos)?;

        // Compression pointer (top two bits set)
        if len & 0xC0 == 0xC0 {
            let lo = *buf.get(pos + 1)?;
            let ptr = (((len & 0x3F) as usize) << 8) | lo as usize;
            if !jumped {
                end = pos + 2;
                jumped = true;
            }
            hops += 1;
            if hops > 32 {
                return None; // malformed / pointer loop
            }
            pos = ptr;
            continue;
        }

        if len == 0 {
            if !jumped {
                end = pos + 1;
            }
            break;
        }

        let s = pos + 1;
        let e = s.checked_add(len as usize)?;
        if e > buf.len() {
            return None;
        }
        labels.push(String::from_utf8_lossy(&buf[s..e]).to_ascii_lowercase());
        pos = e;
    }

    Some((labels.join("."), end))
}

/// Parse a DNS query payload (starting at the DNS header).
/// Returns None for anything that is not a single-question query.
pub fn parse_query(buf: &[u8]) -> Option<DnsQuery> {
    if buf.len() < 12 {
        return None;
    }
    let flags = u16::from_be_bytes([buf[2], buf[3]]);
    let is_response = flags & 0x8000 != 0;
    if is_response {
        return None;
    }
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    if qd != 1 {
        return None;
    }
    let (name, pos) = read_name(buf, 12)?;
    if pos + 4 > buf.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
    let _qclass = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]);
    if name.is_empty() {
        return None;
    }
    Some(DnsQuery {
        id: u16::from_be_bytes([buf[0], buf[1]]),
        flags,
        name,
        qtype,
    })
}

/// Append a dotted name as DNS labels.
fn push_name(out: &mut Vec<u8>, name: &str) {
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            continue;
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
}

/// Build a DNS response answering the query with the given A records
/// (network byte order). An empty `ips` list produces a NOERROR / no-answer
/// response, which makes the resolver fall back to the other address family.
pub fn build_response(q: &DnsQuery, ips: &[u32], ttl: u32) -> Vec<u8> {
    let rd = q.flags & 0x0100 != 0;
    let mut out = Vec::with_capacity(12 + q.name.len() + 2 * 16 + 16 * ips.len().max(1));
    out.extend_from_slice(&q.id.to_be_bytes());
    out.extend_from_slice(&[0x81, if rd { 0x80 } else { 0x00 }]); // response, RD echo
    out.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    out.extend_from_slice(&(ips.len() as u16).to_be_bytes()); // ANCOUNT
    out.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    out.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT
    push_name(&mut out, &q.name);
    out.extend_from_slice(&q.qtype.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // IN
    for ip in ips {
        out.extend_from_slice(&[0xC0, 0x0C]); // name: pointer to offset 12
        out.extend_from_slice(&1u16.to_be_bytes()); // A
        out.extend_from_slice(&1u16.to_be_bytes()); // IN
        out.extend_from_slice(&ttl.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH
        out.extend_from_slice(&ip.to_be_bytes());
    }
    out
}

/// Build an upstream A query for `name`.
pub fn build_upstream_query(id: u16, name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + name.len() + 16);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0x01, 0x00]); // query, RD
    out.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    push_name(&mut out, name);
    out.extend_from_slice(&1u16.to_be_bytes()); // A
    out.extend_from_slice(&1u16.to_be_bytes()); // IN
    out
}

/// Parse an upstream response: (A records, min TTL) when the answer's
/// question matches `want_name`. Returns None on mismatch or no A records.
pub fn parse_upstream_response(buf: &[u8], want_name: &str) -> Option<(Vec<u32>, u32)> {
    if buf.len() < 12 {
        return None;
    }
    let flags = u16::from_be_bytes([buf[2], buf[3]]);
    let rcode = flags & 0x000F;
    if flags & 0x8000 == 0 || rcode != 0 {
        return None;
    }
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    if qd == 0 || an == 0 || an > 64 {
        return None;
    }

    let mut pos = 12usize;
    let (qname, next) = read_name(buf, pos)?;
    if qname != want_name.trim_end_matches('.').to_ascii_lowercase() {
        return None;
    }
    pos = next + 4; // QTYPE + QCLASS

    let mut ips = Vec::new();
    let mut min_ttl = u32::MAX;
    for _ in 0..an {
        let (_, next) = read_name(buf, pos)?;
        pos = next;
        if pos + 10 > buf.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let ttl = u32::from_be_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]);
        let rdlen = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;
        let rd_end = pos.checked_add(rdlen)?;
        if rd_end > buf.len() {
            return None;
        }
        if rtype == 1 && rdlen == 4 {
            ips.push(u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]));
            min_ttl = min_ttl.min(ttl);
        }
        pos = rd_end;
    }

    if ips.is_empty() {
        return None;
    }
    Some((ips, min_ttl))
}

/// True if `host` sits inside domain zone `zone` (the zone apex itself counts).
pub fn in_zone(host: &str, zone: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let zone = zone
        .trim_end_matches('.')
        .trim_start_matches('.')
        .trim_start_matches("*.")
        .to_ascii_lowercase();
    if zone.is_empty() {
        return false;
    }
    host == zone || host.ends_with(&format!(".{zone}"))
}

/// Bidirectional fake↔real IPv4 pool with FIFO eviction.
/// One fake IP per real address: re-resolutions of the same address reuse the
/// same fake, so existing connections and the reverse NAT stay consistent.
pub struct FakePool {
    next: u32,
    cap: usize,
    real_to_fake: HashMap<u32, u32>,
    fake_to_real: HashMap<u32, u32>,
    order: VecDeque<u32>, // insertion order of real IPs, for eviction
}

impl FakePool {
    pub fn new(cap: usize) -> Self {
        Self {
            next: POOL_FIRST,
            cap,
            real_to_fake: HashMap::new(),
            fake_to_real: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.real_to_fake.len()
    }

    /// The fake IP standing in for `real` (network order), allocating one
    /// (evicting the oldest mapping when the pool is full) if needed.
    pub fn fake_for_real(&mut self, real: u32) -> u32 {
        if let Some(&fake) = self.real_to_fake.get(&real) {
            return fake;
        }
        if self.real_to_fake.len() >= self.cap {
            // Evict the oldest mapping; its fake IP is handed to the newcomer.
            while let Some(old_real) = self.order.pop_front() {
                if let Some(fake) = self.real_to_fake.remove(&old_real) {
                    self.fake_to_real.remove(&fake);
                    self.real_to_fake.insert(real, fake);
                    self.fake_to_real.insert(fake, real);
                    self.order.push_back(real);
                    return fake;
                }
            }
        }
        let fake = self.next;
        self.next = self.next.wrapping_add(1);
        if self.next > POOL_LAST {
            self.next = POOL_FIRST; // extremely unlikely with a sane cap
        }
        self.real_to_fake.insert(real, fake);
        self.fake_to_real.insert(fake, real);
        self.order.push_back(real);
        fake
    }

    /// The real address behind `fake`, if any.
    pub fn real_for_fake(&self, fake: u32) -> Option<u32> {
        self.fake_to_real.get(&fake).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_matching() {
        assert!(in_zone("example.com", "example.com"));
        assert!(in_zone("cdn.example.com", "example.com"));
        assert!(in_zone("a.b.example.com", "example.com"));
        assert!(in_zone("cdn.example.com", "*.example.com"));
        assert!(!in_zone("notexample.com", "example.com"));
        assert!(!in_zone("example.com.evil.net", "example.com"));
        assert!(!in_zone("example.com", ""));
    }

    #[test]
    fn fake_pool_roundtrip_and_reuse() {
        let mut pool = FakePool::new(4);
        let a = pool.fake_for_real(0x0102_0304);
        let b = pool.fake_for_real(0x0506_0708);
        assert_ne!(a, b);
        assert_eq!(pool.real_for_fake(a), Some(0x0102_0304));
        assert_eq!(pool.real_for_fake(b), Some(0x0506_0708));
        // Same real address reuses the same fake.
        assert_eq!(pool.fake_for_real(0x0102_0304), a);
        // Eviction: with cap 4 the oldest (a's real) is recycled.
        for i in 0..4u32 {
            pool.fake_for_real(0x0A00_0000 + i);
        }
        assert!(pool.real_for_fake(a).is_none() || pool.real_for_fake(a) != Some(0x0102_0304));
    }

    #[test]
    fn query_parse_and_response_roundtrip() {
        let mut q: Vec<u8> = vec![0xAB, 0xCD, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(&[3]);
        q.extend_from_slice(b"foo");
        q.extend_from_slice(&[2]);
        q.extend_from_slice(b"ru");
        q.push(0);
        q.extend_from_slice(&[0, 1, 0, 1]);

        let parsed = parse_query(&q).expect("query parses");
        assert_eq!(parsed.id, 0xABCD);
        assert_eq!(parsed.name, "foo.ru");
        assert_eq!(parsed.qtype, 1);

        let resp = build_response(&parsed, &[0xC612_000A], 30);
        let back = parse_upstream_response(&resp, "foo.ru").expect("response parses");
        assert_eq!(back.0, vec![0xC612_000A]);
        assert_eq!(back.1, 30);
    }

    #[test]
    fn upstream_query_is_parseable() {
        let q = build_upstream_query(7, "example.org");
        let parsed = parse_query(&q).expect("built query parses");
        assert_eq!(parsed.name, "example.org");
        assert_eq!(parsed.qtype, 1);
    }
}
