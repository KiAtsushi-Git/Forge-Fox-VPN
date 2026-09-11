/// Minimal userspace-TCP packet crafting for bypass flows.
///
/// When a flow is bypassed (re-originated outside the VPN through a
/// protected socket) the core terminates TCP itself: it answers the app's
/// SYN, relays data and closes — acting as the remote endpoint. This module
/// holds the pure packet math (header parse/build, checksums, sequence
/// arithmetic). It is host-testable, like `dns`; the state machine and
/// sockets live in `ssh_vpn`.

pub const FIN: u8 = 0x01;
pub const SYN: u8 = 0x02;
pub const RST: u8 = 0x04;
pub const PSH: u8 = 0x08;
pub const ACK: u8 = 0x10;

/// Options advertised in our SYN-ACK.
pub const OUR_MSS: u16 = 1300;
/// Our window-scale exponent: advertised window 0xFFFF << 7 = 512 KiB.
pub const OUR_WSCALE: u8 = 7;

/// Sequence-number ordering: `a < b` in TCP's wrapping space.
pub fn seq_lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

pub fn seq_le(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) <= 0
}

/// One parsed TCP header (from a full IPv4 packet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpSegment {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    /// FIN|SYN|RST|PSH|ACK bit mask.
    pub flags: u8,
    /// Raw advertised window (before scaling).
    pub window: u16,
    /// Peer's window-scale exponent, if it sent one (SYN only).
    pub wscale: Option<u8>,
    /// Peer's MSS, if it sent one (SYN only).
    pub mss: Option<u16>,
    /// Byte offset of the payload inside the packet (== len when empty).
    pub payload_off: usize,
}

impl TcpSegment {
    pub fn payload<'a>(&self, pkt: &'a [u8]) -> &'a [u8] {
        &pkt[self.payload_off..]
    }
}

/// Parse the TCP header out of an IPv4 packet. Returns None for anything
/// malformed or non-TCP.
pub fn parse_tcp(pkt: &[u8]) -> Option<TcpSegment> {
    if pkt.len() < 20 || pkt[0] >> 4 != 4 {
        return None;
    }
    let ihl = ((pkt[0] & 0x0F) as usize) * 4;
    if ihl < 20 || pkt.len() < ihl + 20 || pkt[9] != 6 {
        return None;
    }
    let src_ip = u32::from_be_bytes([pkt[12], pkt[13], pkt[14], pkt[15]]);
    let dst_ip = u32::from_be_bytes([pkt[16], pkt[17], pkt[18], pkt[19]]);
    let t = &pkt[ihl..];
    let src_port = u16::from_be_bytes([t[0], t[1]]);
    let dst_port = u16::from_be_bytes([t[2], t[3]]);
    let seq = u32::from_be_bytes([t[4], t[5], t[6], t[7]]);
    let ack = u32::from_be_bytes([t[8], t[9], t[10], t[11]]);
    let doff = (t[12] >> 4) as usize;
    if doff < 5 || ihl + doff * 4 > pkt.len() {
        return None;
    }
    let flags = t[13];
    let window = u16::from_be_bytes([t[14], t[15]]);

    // Options: only MSS (kind 2) and window scale (kind 3) matter to us.
    let opts_end = ihl + doff * 4;
    let mut mss = None;
    let mut wscale = None;
    let mut o = ihl + 20;
    while o < opts_end {
        let kind = pkt[o];
        if kind == 0 {
            break; // EOL
        }
        if kind == 1 {
            o += 1; // NOP
            continue;
        }
        let Some(&len) = pkt.get(o + 1) else { break };
        if len < 2 || o + len as usize > opts_end {
            break;
        }
        match kind {
            2 if len == 4 => {
                mss = Some(u16::from_be_bytes([pkt[o + 2], pkt[o + 3]]));
            }
            3 if len == 3 => {
                wscale = Some(pkt[o + 2]);
            }
            _ => {}
        }
        o += len as usize;
    }

    Some(TcpSegment {
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        seq,
        ack,
        flags,
        window,
        wscale,
        mss,
        payload_off: opts_end,
    })
}

/// Raw one's-complement sum of `data`, folded to 16 bits (no complement).
fn ocs(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    sum
}

/// Standard Internet checksum of `data` (complemented one's-complement sum).
fn checksum16(data: &[u8]) -> u16 {
    !(ocs(data) as u16)
}

/// TCP checksum over the IPv4 pseudo-header + segment. With a correct
/// checksum embedded this returns 0 (verification), with a zero checksum
/// field it returns the value to embed.
fn tcp_checksum(src: u32, dst: u32, seg: &[u8]) -> u16 {
    let ph = [
        (src >> 24) as u8,
        (src >> 16) as u8,
        (src >> 8) as u8,
        src as u8,
        (dst >> 24) as u8,
        (dst >> 16) as u8,
        (dst >> 8) as u8,
        dst as u8,
        0,
        6, // TCP
        (seg.len() >> 8) as u8,
        seg.len() as u8,
    ];
    let mut sum = ocs(&ph) + ocs(seg);
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn fix_ipv4_checksum(hdr: &mut [u8]) {
    let ck = {
        hdr[10] = 0;
        hdr[11] = 0;
        checksum16(&hdr[..20])
    };
    hdr[10..12].copy_from_slice(&ck.to_be_bytes());
}

/// Options block for a SYN-ACK: MSS + window scale, NOP-padded to 4 bytes.
fn synack_options() -> [u8; 8] {
    let mut o = [1u8; 8];
    o[0] = 2; // MSS
    o[1] = 4;
    o[2..4].copy_from_slice(&OUR_MSS.to_be_bytes());
    o[4] = 3; // WS
    o[5] = 3; // len
    o[6] = OUR_WSCALE;
    o
}

/// Craft a full IPv4+TCP packet from the "remote endpoint" back to the app.
///
/// `src`/`dst` are the bypassed flow's real destination and the app's TUN
/// address respectively. TCP checksum is computed properly (0 is illegal for
/// TCP, unlike IPv4 UDP).
pub fn build_tcp_packet(
    src: u32,
    dst: u32,
    sport: u16,
    dport: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
) -> Vec<u8> {
    let with_syn_opts = flags & SYN != 0;
    let opt_len = if with_syn_opts { synack_options().len() } else { 0 };
    let tcp_len = 20 + opt_len + payload.len();
    let total = 20 + tcp_len;

    let mut pkt = Vec::with_capacity(total);
    // IPv4 header
    pkt.extend_from_slice(&[0x45, 0x00]);
    pkt.extend_from_slice(&(total as u16).to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ID
    pkt.extend_from_slice(&0x4000u16.to_be_bytes()); // DF, no fragment
    pkt.push(64); // TTL
    pkt.push(6); // TCP
    pkt.extend_from_slice(&[0, 0]); // checksum placeholder
    pkt.extend_from_slice(&src.to_be_bytes());
    pkt.extend_from_slice(&dst.to_be_bytes());
    // TCP header
    pkt.extend_from_slice(&sport.to_be_bytes());
    pkt.extend_from_slice(&dport.to_be_bytes());
    pkt.extend_from_slice(&seq.to_be_bytes());
    pkt.extend_from_slice(&ack.to_be_bytes());
    pkt.push((((20 + opt_len) / 4) << 4) as u8); // data offset
    pkt.push(flags);
    pkt.extend_from_slice(&window.to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
    pkt.extend_from_slice(&0u16.to_be_bytes()); // urgent ptr
    if with_syn_opts {
        pkt.extend_from_slice(&synack_options());
    }
    pkt.extend_from_slice(payload);

    // checksums
    let tcp_ck = tcp_checksum(src, dst, &pkt[20..]);
    pkt[20 + 16..20 + 18].copy_from_slice(&tcp_ck.to_be_bytes());
    fix_ipv4_checksum(&mut pkt[..20]);
    pkt
}

/// Verify the TCP checksum of a full IPv4 packet (test helper, also used as
/// a sanity check on crafted packets).
pub fn tcp_checksum_ok(pkt: &[u8]) -> bool {
    let Some(seg) = parse_tcp(pkt) else {
        return false;
    };
    let sum = tcp_checksum(seg.src_ip, seg.dst_ip, &pkt[20..]);
    sum == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(a: u8, b: u8, c: u8, d: u8) -> u32 {
        u32::from_be_bytes([a, b, c, d])
    }

    /// Build a client→server packet (as if read from the TUN), used to test
    /// round-trips. Checksum left zero — parse_tcp ignores it.
    fn client_pkt(flags: u8, seq: u32, ack: u32, payload: &[u8], opts: &[u8]) -> Vec<u8> {
        let tcp_len = 20 + opts.len() + payload.len();
        let total = 20 + tcp_len;
        let mut pkt = vec![0u8; total];
        pkt[0] = 0x45;
        pkt[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        pkt[8] = 64;
        pkt[9] = 6;
        pkt[12..16].copy_from_slice(&ip(10, 9, 8, 7).to_be_bytes()); // app
        pkt[16..20].copy_from_slice(&ip(93, 184, 216, 34).to_be_bytes()); // real dst
        let t = &mut pkt[20..];
        t[0..2].copy_from_slice(&51000u16.to_be_bytes());
        t[2..4].copy_from_slice(&443u16.to_be_bytes());
        t[4..8].copy_from_slice(&seq.to_be_bytes());
        t[8..12].copy_from_slice(&ack.to_be_bytes());
        t[12] = (((20 + opts.len()) / 4) << 4) as u8;
        t[13] = flags;
        t[14..16].copy_from_slice(&0xFCE4u16.to_be_bytes()); // window 64740
        t[20..20 + opts.len()].copy_from_slice(opts);
        pkt[20 + 20 + opts.len()..].copy_from_slice(payload);
        pkt
    }

    #[test]
    fn parse_syn_with_options() {
        // SYN with MSS 1460 and WS 7 (NOPs first to exercise the option walk;
        // options padded to a 4-byte boundary like a real stack sends them)
        let opts = [1u8, 1, 2, 4, 0x05, 0xB4, 3, 3, 7, 1, 1, 1];
        let pkt = client_pkt(SYN, 1000, 0, &[], &opts);
        let seg = parse_tcp(&pkt).unwrap();
        assert_eq!(seg.flags, SYN);
        assert_eq!(seg.seq, 1000);
        assert_eq!(seg.mss, Some(1460));
        assert_eq!(seg.wscale, Some(7));
        assert_eq!(seg.window, 64740);
        assert_eq!(seg.src_ip, ip(10, 9, 8, 7));
        assert_eq!(seg.dst_ip, ip(93, 184, 216, 34));
        assert_eq!(seg.src_port, 51000);
        assert_eq!(seg.dst_port, 443);
        assert!(seg.payload(&pkt).is_empty());
    }

    #[test]
    fn parse_data_segment() {
        let pkt = client_pkt(PSH | ACK, 2000, 9000, b"hello", &[]);
        let seg = parse_tcp(&pkt).unwrap();
        assert_eq!(seg.seq, 2000);
        assert_eq!(seg.ack, 9000);
        assert_eq!(seg.flags, PSH | ACK);
        assert_eq!(seg.payload(&pkt), b"hello");
    }

    #[test]
    fn build_parse_roundtrip_with_valid_checksum() {
        let pkt = build_tcp_packet(
            ip(93, 184, 216, 34),
            ip(10, 9, 8, 7),
            443,
            51000,
            9000,
            2001,
            PSH | ACK,
            0xFFFF,
            b"world!",
        );
        assert!(tcp_checksum_ok(&pkt));
        let seg = parse_tcp(&pkt).unwrap();
        assert_eq!(seg.seq, 9000);
        assert_eq!(seg.ack, 2001);
        assert_eq!(seg.payload(&pkt), b"world!");
        assert_eq!(seg.src_port, 443);
        assert_eq!(seg.dst_port, 51000);
        assert_eq!(seg.window, 0xFFFF);
    }

    #[test]
    fn synack_carries_mss_and_wscale() {
        let pkt = build_tcp_packet(
            ip(1, 2, 3, 4),
            ip(10, 0, 0, 2),
            80,
            40000,
            500,
            7001,
            SYN | ACK,
            0xFFFF,
            &[],
        );
        assert!(tcp_checksum_ok(&pkt));
        let seg = parse_tcp(&pkt).unwrap();
        assert_eq!(seg.flags, SYN | ACK);
        assert_eq!(seg.mss, Some(OUR_MSS));
        assert_eq!(seg.wscale, Some(OUR_WSCALE));
        // non-SYN packets must not carry options
        let plain = build_tcp_packet(1, 2, 80, 40000, 5, 6, ACK, 100, &[]);
        assert_eq!(parse_tcp(&plain).unwrap().mss, None);
        assert!(tcp_checksum_ok(&plain));
    }

    #[test]
    fn fin_and_rst_packets() {
        for flags in [FIN | ACK, RST, RST | ACK] {
            let pkt = build_tcp_packet(1, 2, 80, 40000, 5, 6, flags, 100, &[]);
            assert!(tcp_checksum_ok(&pkt), "flags {flags}");
            assert_eq!(parse_tcp(&pkt).unwrap().flags, flags);
        }
    }

    #[test]
    fn sequence_wraparound_ordering() {
        assert!(seq_lt(0xFFFF_FFF0, 0x0000_0010)); // across wrap
        assert!(!seq_lt(0x0000_0010, 0xFFFF_FFF0));
        assert!(seq_le(5, 5));
        assert!(!seq_lt(5, 5));
        assert!(!seq_lt(100, 50));
    }

    #[test]
    fn rejects_non_tcp_and_truncated() {
        let mut pkt = vec![0u8; 48];
        pkt[0] = 0x45;
        pkt[9] = 17; // UDP
        assert!(parse_tcp(&pkt).is_none());
        pkt[9] = 6;
        pkt[12] = 0x46; // ihl=6 → opts; doff default 5 → fine, but header says 6*4=24
        pkt[12] = 0x45; // reset
        let short = &pkt[..30];
        assert!(parse_tcp(short).is_none());
    }
}
