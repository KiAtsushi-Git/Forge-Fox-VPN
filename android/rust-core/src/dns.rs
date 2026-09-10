/// Minimal DNS response parser.
///
/// Ported from the desktop client (windows/src-tauri/src/vpn/dns.rs) so the
/// Android core can learn IPs behind `domain` / `domain_zone` rules as they
/// are resolved, instead of relying on a single up-front lookup (CDNs rotate
/// addresses constantly).
use std::net::Ipv4Addr;

#[derive(Debug, Default)]
pub struct DnsAnswer {
    /// Lowercased question name, e.g. "cdn.example.com"
    pub query: String,
    /// Every name seen in the answer chain (question + CNAME targets)
    pub names: Vec<String>,
    /// A-record addresses
    pub ips: Vec<Ipv4Addr>,
}

/// Read a (possibly compressed) DNS name starting at `pos`.
/// Returns the decoded name and the offset just past the name in the *original*
/// position (compression pointers do not advance the outer cursor).
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

/// Parse a DNS message body (starting at the DNS header).
/// Returns `None` for queries, malformed data, or answers with no useful records.
pub fn parse_response(buf: &[u8]) -> Option<DnsAnswer> {
    if buf.len() < 12 {
        return None;
    }

    let flags = u16::from_be_bytes([buf[2], buf[3]]);
    let is_response = flags & 0x8000 != 0;
    let rcode = flags & 0x000F;
    if !is_response || rcode != 0 {
        return None;
    }

    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    if qd == 0 || an == 0 || an > 64 {
        return None;
    }

    let mut out = DnsAnswer::default();
    let mut pos = 12usize;

    // ── Questions ─────────────────────────────────────────────────────────────
    for i in 0..qd {
        let (name, next) = read_name(buf, pos)?;
        pos = next.checked_add(4)?; // QTYPE + QCLASS
        if pos > buf.len() {
            return None;
        }
        if i == 0 {
            out.query = name.clone();
        }
        if !name.is_empty() {
            out.names.push(name);
        }
    }

    // ── Answers ───────────────────────────────────────────────────────────────
    for _ in 0..an {
        let (name, next) = read_name(buf, pos)?;
        pos = next;
        if pos + 10 > buf.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let rdlen = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;
        let rd_end = pos.checked_add(rdlen)?;
        if rd_end > buf.len() {
            return None;
        }

        if !name.is_empty() && !out.names.contains(&name) {
            out.names.push(name);
        }

        match rtype {
            // A
            1 if rdlen == 4 => {
                out.ips.push(Ipv4Addr::new(
                    buf[pos],
                    buf[pos + 1],
                    buf[pos + 2],
                    buf[pos + 3],
                ));
            }
            // CNAME — follow the alias chain so zone matching still applies
            5 => {
                if let Some((cname, _)) = read_name(buf, pos) {
                    if !cname.is_empty() && !out.names.contains(&cname) {
                        out.names.push(cname);
                    }
                }
            }
            _ => {}
        }

        pos = rd_end;
    }

    if out.ips.is_empty() {
        return None;
    }
    Some(out)
}

/// Extract a DNS response from a raw IPv4 packet, if it is one
/// (UDP, source port 53). Returns `None` for anything else.
pub fn parse_ipv4_dns_packet(pkt: &[u8]) -> Option<DnsAnswer> {
    if pkt.len() < 20 {
        return None;
    }
    // IPv4 only
    if pkt[0] >> 4 != 4 {
        return None;
    }
    let ihl = ((pkt[0] & 0x0F) as usize) * 4;
    if ihl < 20 || pkt.len() < ihl + 8 {
        return None;
    }
    // UDP
    if pkt[9] != 17 {
        return None;
    }
    // Fragmented packets: only the first fragment carries the UDP header
    let frag = u16::from_be_bytes([pkt[6], pkt[7]]) & 0x1FFF;
    if frag != 0 {
        return None;
    }

    let udp = &pkt[ihl..];
    let src_port = u16::from_be_bytes([udp[0], udp[1]]);
    if src_port != 53 {
        return None;
    }

    let udp_len = u16::from_be_bytes([udp[4], udp[5]]) as usize;
    if udp_len < 8 || udp_len > udp.len() {
        return None;
    }

    parse_response(&udp[8..udp_len])
}

/// True if `host` sits inside domain zone `zone` (the zone apex itself counts).
pub fn in_zone(host: &str, zone: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let zone = zone.trim_end_matches('.').trim_start_matches('.')
        .trim_start_matches("*.")
        .to_ascii_lowercase();
    if zone.is_empty() {
        return false;
    }
    host == zone || host.ends_with(&format!(".{zone}"))
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
    fn parses_simple_a_response() {
        // ID, flags(response), qd=1, an=1, ns=0, ar=0
        let mut p: Vec<u8> = vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0];
        // question: example.com A IN
        p.extend_from_slice(&[7]);
        p.extend_from_slice(b"example");
        p.extend_from_slice(&[3]);
        p.extend_from_slice(b"com");
        p.push(0);
        p.extend_from_slice(&[0, 1, 0, 1]);
        // answer: ptr to 12, A, IN, ttl, rdlen 4, 1.2.3.4
        p.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 1, 2, 3, 4]);

        let a = parse_response(&p).expect("should parse");
        assert_eq!(a.query, "example.com");
        assert_eq!(a.ips, vec![Ipv4Addr::new(1, 2, 3, 4)]);
    }

    #[test]
    fn rejects_query_and_garbage() {
        let q: Vec<u8> = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        assert!(parse_response(&q).is_none());
        assert!(parse_response(&[0u8; 4]).is_none());
    }
}
