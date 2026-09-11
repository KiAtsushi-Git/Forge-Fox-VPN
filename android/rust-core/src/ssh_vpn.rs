use jni::objects::{GlobalRef, JClass, JString};
use jni::sys::jint;
use jni::JNIEnv;
use serde_json::Value;
use std::os::unix::io::{RawFd, AsRawFd};
use tokio::runtime::Runtime;
use std::sync::Arc;
use russh::*;
use russh_keys::*;
use std::io;
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::Mutex;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::dns;
use crate::flow_rules::{self, FlowKey, FlowTable, RangeSet, Route, PROTO_TCP, PROTO_UDP};
use crate::tcp_gen;

static CANCEL_TX: Mutex<Option<tokio::sync::broadcast::Sender<()>>> = Mutex::new(None);

/// JVM handle + a global reference to the Core class, captured on the JNI
/// entry thread so background tasks can call `Core.protectFd` later (the
/// upstream DNS sockets must bypass the VPN, like the SSH socket itself).
static JAVA_VM: Mutex<Option<jni::JavaVM>> = Mutex::new(None);
static CORE_CLASS: Mutex<Option<GlobalRef>> = Mutex::new(None);

extern "C" {
    pub fn __android_log_print(prio: libc::c_int, tag: *const libc::c_char, fmt: *const libc::c_char, ...) -> libc::c_int;
}


macro_rules! log_d {
    ($($arg:tt)*) => {{
        if let Ok(msg) = std::ffi::CString::new(format!($($arg)*)) {
            unsafe {
                __android_log_print(
                    3, // ANDROID_LOG_DEBUG
                    b"RustVpn\0".as_ptr() as *const libc::c_char,
                    b"%s\0".as_ptr() as *const libc::c_char,
                    msg.as_ptr(),
                );
            }
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/data/data/com.forgefox.vpn/cache/rust_vpn.log")
            .map(|mut f| {
                use std::io::Write;
                let _ = writeln!(f, "[DEBUG] {}", format!($($arg)*));
            });
    }};
}

macro_rules! log_e {
    ($($arg:tt)*) => {{
        if let Ok(msg) = std::ffi::CString::new(format!($($arg)*)) {
            unsafe {
                __android_log_print(
                    6, // ANDROID_LOG_ERROR
                    b"RustVpn\0".as_ptr() as *const libc::c_char,
                    b"%s\0".as_ptr() as *const libc::c_char,
                    msg.as_ptr(),
                );
            }
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/data/data/com.forgefox.vpn/cache/rust_vpn.log")
            .map(|mut f| {
                use std::io::Write;
                let _ = writeln!(f, "[ERROR] {}", format!($($arg)*));
            });
    }};
}
// TUN file descriptor async wrapper
pub struct TunFd(AsyncFd<RawFd>);

impl TunFd {
    pub fn new(fd: RawFd) -> io::Result<Self> {
        unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) };
        Ok(Self(AsyncFd::new(fd)?))
    }
}

impl AsyncRead for TunFd {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        loop {
            let mut guard = futures::ready!(self.0.poll_read_ready(cx))?;
            match guard.try_io(|inner| {
                let res = unsafe { libc::read(*inner.get_ref(), buf.unfilled_mut().as_mut_ptr() as *mut _, buf.remaining()) };
                if res < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(res as usize)
                }
            }) {
                Ok(Ok(n)) => {
                    unsafe { buf.assume_init(n) };
                    buf.advance(n);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(e)) => return Poll::Ready(Err(e)),
                Err(_would_block) => continue,
            }
        }
    }
}

impl AsyncWrite for TunFd {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        loop {
            let mut guard = futures::ready!(self.0.poll_write_ready(cx))?;
            match guard.try_io(|inner| {
                let res = unsafe { libc::write(*inner.get_ref(), buf.as_ptr() as *const _, buf.len()) };
                if res < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(res as usize)
                }
            }) {
                Ok(res) => return Poll::Ready(res),
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct ClientHandler;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;
    async fn check_server_key(
        self,
        _server_public_key: &russh_keys::key::PublicKey,
    ) -> Result<(Self, bool), Self::Error> {
        Ok((self, true)) // Accept all for testing
    }
}

// ── Fake-IP DNS interception (Proxy mode + wildcard zones) ────────────────────

/// Protect a socket fd so its traffic bypasses the VPN (physical network).
/// Runs on tokio worker threads: attach to the JVM, call Core.protectFd.
fn protect_fd(fd: RawFd) -> bool {
    let vm_lock = match JAVA_VM.lock() {
        Ok(l) => l,
        Err(_) => return false,
    };
    let class_lock = match CORE_CLASS.lock() {
        Ok(l) => l,
        Err(_) => return false,
    };
    let (Some(vm), Some(class_ref)) = (vm_lock.as_ref(), class_lock.as_ref()) else {
        return false;
    };

    let mut env = match vm.attach_current_thread() {
        Ok(guard) => guard,
        Err(e) => {
            log_e!("JVM attach failed: {e:?}");
            return false;
        }
    };
    // Safety: the raw pointer is a JNI global reference owned by `class_ref`,
    // which stays alive for the duration of the call.
    let class = unsafe { JClass::from_raw(class_ref.as_raw() as _) };
    env.call_static_method(class, "protectFd", "(I)Z", &[jni::objects::JValue::Int(fd)])
        .map(|r| r.z().unwrap_or(false))
        .unwrap_or(false)
}

/// Call a static method on the cached Core class, returning a jint.
fn jni_call_static_int(name: &str, sig: &str, args: &[jni::objects::JValue]) -> Option<i32> {
    let vm_lock = JAVA_VM.lock().ok()?;
    let class_lock = CORE_CLASS.lock().ok()?;
    let vm = vm_lock.as_ref()?;
    let class_ref = class_lock.as_ref()?;
    let mut env = vm.attach_current_thread().ok()?;
    // Safety: the raw pointer is a JNI global reference owned by `class_ref`,
    // which stays alive for the duration of the call.
    let class = unsafe { JClass::from_raw(class_ref.as_raw() as _) };
    env.call_static_method(class, name, sig, args)
        .ok()?
        .i()
        .ok()
}

/// Ask Java (ConnectivityManager.getConnectionOwnerUid, API 29+) which uid
/// owns the connection (proto, src, sport) → (dst, dport). -1 = unknown.
fn jni_get_connection_owner(proto: u8, src: u32, sport: u16, dst: u32, dport: u16) -> Option<i32> {
    jni_call_static_int(
        "getConnectionOwner",
        "(IIIII)I",
        &[
            jni::objects::JValue::Int(proto as i32),
            jni::objects::JValue::Int(src as i32),
            jni::objects::JValue::Int(sport as i32),
            jni::objects::JValue::Int(dst as i32),
            jni::objects::JValue::Int(dport as i32),
        ],
    )
}

/// True when the uid belongs to one of the apps selected for the VPN
/// (Proxy split mode). False for everyone else, including ourselves.
fn jni_is_app_selected(uid: i32) -> bool {
    let vm_lock = match JAVA_VM.lock() {
        Ok(l) => l,
        Err(_) => return false,
    };
    let class_lock = match CORE_CLASS.lock() {
        Ok(l) => l,
        Err(_) => return false,
    };
    let (Some(vm), Some(class_ref)) = (vm_lock.as_ref(), class_lock.as_ref()) else {
        return false;
    };
    let mut env = match vm.attach_current_thread() {
        Ok(guard) => guard,
        Err(_) => return false,
    };
    // Safety: the raw pointer is a JNI global reference owned by `class_ref`,
    // which stays alive for the duration of the call.
    let class = unsafe { JClass::from_raw(class_ref.as_raw() as _) };
    env.call_static_method(class, "isAppSelected", "(I)Z", &[jni::objects::JValue::Int(uid)])
        .map(|r| r.z().unwrap_or(false))
        .unwrap_or(false)
}

/// Decide whether a new flow belongs to a selected app. Retries briefly:
/// right after a SYN/first-packet the kernel socket may not be visible to
/// the connectivity service yet. `None` = could not determine the owner —
/// callers treat that as "tunnel" (safe: traffic still flows).
async fn lookup_owner_selected(proto: u8, src: u32, sport: u16, dst: u32, dport: u16) -> Option<bool> {
    for _ in 0..3 {
        if let Some(uid) = jni_get_connection_owner(proto, src, sport, dst, dport) {
            if uid >= 0 {
                return Some(jni_is_app_selected(uid));
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

/// Recompute the IPv4 header checksum in place (first 20 header bytes).
fn fix_ipv4_checksum(hdr: &mut [u8]) {
    hdr[10] = 0;
    hdr[11] = 0;
    let mut sum: u32 = 0;
    for c in hdr[..20].chunks(2) {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let ck = !(sum as u16);
    hdr[10] = (ck >> 8) as u8;
    hdr[11] = (ck & 0xFF) as u8;
}

/// RFC 1624 incremental checksum update after a 32-bit field (an IP address)
/// changed from `old` to `new`, both in network byte order.
fn adjust_l4_checksum(ck: u16, old: u32, new: u32) -> u16 {
    let words = |v: u32| [(v >> 16) as u16, v as u16];
    let mut sum: u32 = (!ck) as u32;
    for w in words(old) {
        let inv = !w; // invert in 16 bits, then widen
        sum = sum.wrapping_add(inv as u32);
    }
    for w in words(new) {
        sum = sum.wrapping_add(w as u32);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Fix the TCP/UDP checksum after an address rewrite (0 stays 0: "no
/// checksum" is legal for IPv4 UDP, invalid-but-unused for TCP).
fn fix_l4_for_addr(pkt: &mut [u8], old: u32, new: u32) {
    let ihl = ((pkt[0] & 0x0F) as usize) * 4;
    let proto = pkt[9];
    let off = match proto {
        6 => ihl + 16, // TCP
        17 => ihl + 6, // UDP
        _ => return,
    };
    if pkt.len() < off + 2 {
        return;
    }
    let ck = u16::from_be_bytes([pkt[off], pkt[off + 1]]);
    if ck == 0 && proto == 17 {
        return;
    }
    let ck = adjust_l4_checksum(ck, old, new);
    pkt[off] = (ck >> 8) as u8;
    pkt[off + 1] = (ck & 0xFF) as u8;
}

/// Rewrite the IPv4 destination address, fixing header + L4 checksums.
fn rewrite_dst(pkt: &mut [u8], new_dst: u32) {
    let old = u32::from_be_bytes([pkt[16], pkt[17], pkt[18], pkt[19]]);
    pkt[16..20].copy_from_slice(&new_dst.to_be_bytes());
    fix_ipv4_checksum(&mut pkt[..20]);
    fix_l4_for_addr(pkt, old, new_dst);
}

/// Rewrite the IPv4 source address, fixing header + L4 checksums.
fn rewrite_src(pkt: &mut [u8], new_src: u32) {
    let old = u32::from_be_bytes([pkt[12], pkt[13], pkt[14], pkt[15]]);
    pkt[12..16].copy_from_slice(&new_src.to_be_bytes());
    fix_ipv4_checksum(&mut pkt[..20]);
    fix_l4_for_addr(pkt, old, new_src);
}

/// What the bridge should do with an outbound TUN packet.
enum OutAction {
    /// Forward to the SSH stream (after fake→real NAT when applicable).
    Forward,
    /// Hand to the DNS interceptor: (DNS payload, client ip, client port).
    Dns(Vec<u8>, u32, u16),
    /// Swallow (fake IP with no mapping).
    Drop,
}

/// Fake-IP engine: intercepts DNS to the virtual server, hands out fake IPs
/// for zone-matching domains and NATs fake↔real on the TUN↔SSH path.
struct DnsEngine {
    enabled: bool,
    zones: Vec<String>,
    server_ip: u32,
    upstream: std::net::SocketAddr,
    pool: dns::FakePool,
    /// (proto, remote ip, remote port, client ip, client port) → fake ip,
    /// per-flow so only connections that actually dialed a fake IP get
    /// their inbound packets rewritten back.
    conntrack: HashMap<(u8, u32, u16, u32, u16), u32>,
    conn_order: VecDeque<(u8, u32, u16, u32, u16)>,
}

impl DnsEngine {
    fn from_settings(settings: &Value) -> Self {
        let zones: Vec<String> = settings["dns_zones"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| {
                        s.trim()
                            .trim_start_matches("*.")
                            .to_ascii_lowercase()
                    })
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let enabled = !zones.is_empty();
        let server_ip = settings["dns_server_ip"]
            .as_str()
            .unwrap_or("198.18.0.2")
            .parse::<std::net::Ipv4Addr>()
            .map(u32::from)
            .unwrap_or(0xC612_0002);
        let upstream = settings["dns_upstream"]
            .as_str()
            .unwrap_or("8.8.8.8:53")
            .to_string()
            .parse()
            .unwrap_or_else(|_| "8.8.8.8:53".parse().unwrap());
        Self {
            enabled,
            zones,
            server_ip,
            upstream,
            pool: dns::FakePool::new(8192),
            conntrack: HashMap::new(),
            conn_order: VecDeque::new(),
        }
    }

    /// TUN → SSH direction: DNS interception, fake→real NAT.
    fn process_outbound(&mut self, pkt: &mut [u8]) -> OutAction {
        if !self.enabled {
            return OutAction::Forward;
        }
        if pkt.len() < 20 || pkt[0] >> 4 != 4 {
            return OutAction::Forward;
        }
        let ihl = ((pkt[0] & 0x0F) as usize) * 4;
        if ihl < 20 || pkt.len() < ihl {
            return OutAction::Forward;
        }
        let proto = pkt[9];
        let src = u32::from_be_bytes([pkt[12], pkt[13], pkt[14], pkt[15]]);
        let dst = u32::from_be_bytes([pkt[16], pkt[17], pkt[18], pkt[19]]);

        // DNS query to the virtual server → handle locally, never forward.
        if proto == 17 && pkt.len() >= ihl + 8 {
            let sport = u16::from_be_bytes([pkt[ihl], pkt[ihl + 1]]);
            let dport = u16::from_be_bytes([pkt[ihl + 2], pkt[ihl + 3]]);
            if dst == self.server_ip && dport == 53 {
                return OutAction::Dns(pkt[ihl + 8..].to_vec(), src, sport);
            }
        }

        if !dns::is_fake_ip(dst) {
            return OutAction::Forward;
        }
        let Some(real) = self.pool.real_for_fake(dst) else {
            return OutAction::Drop; // stale fake IP after a pool eviction
        };
        if (proto == 6 || proto == 17) && pkt.len() >= ihl + 4 {
            let sport = u16::from_be_bytes([pkt[ihl], pkt[ihl + 1]]);
            let dport = u16::from_be_bytes([pkt[ihl + 2], pkt[ihl + 3]]);
            // The destination port is untouched by the NAT, so the flow key
            // is already the post-NAT (real) tuple.
            let key = (proto, real, dport, src, sport);
            if !self.conntrack.contains_key(&key) {
                if self.conntrack.len() >= 16384 {
                    while let Some(old) = self.conn_order.pop_front() {
                        if self.conntrack.remove(&old).is_some() {
                            break;
                        }
                    }
                }
                self.conntrack.insert(key, dst);
                self.conn_order.push_back(key);
            }
        }
        rewrite_dst(pkt, real);
        OutAction::Forward
    }

    /// SSH → TUN direction: real→fake NAT for tracked flows.
    fn process_inbound(&mut self, pkt: &mut [u8]) {
        if !self.enabled {
            return;
        }
        if pkt.len() < 20 || pkt[0] >> 4 != 4 {
            return;
        }
        let ihl = ((pkt[0] & 0x0F) as usize) * 4;
        if ihl < 20 || pkt.len() < ihl + 4 {
            return;
        }
        let proto = pkt[9];
        if proto != 6 && proto != 17 {
            return;
        }
        let src = u32::from_be_bytes([pkt[12], pkt[13], pkt[14], pkt[15]]);
        let dst = u32::from_be_bytes([pkt[16], pkt[17], pkt[18], pkt[19]]);
        let sport = u16::from_be_bytes([pkt[ihl], pkt[ihl + 1]]);
        let dport = u16::from_be_bytes([pkt[ihl + 2], pkt[ihl + 3]]);
        if let Some(&fake) = self.conntrack.get(&(proto, src, sport, dst, dport)) {
            rewrite_src(pkt, fake);
        }
    }
}

/// Craft an IPv4+UDP packet (real header checksum, UDP checksum 0 — legal
/// for IPv4) carrying `payload`.
fn build_udp_packet(src: u32, dst: u32, sport: u16, dport: u16, payload: &[u8]) -> Vec<u8> {
    let total = 20 + 8 + payload.len();
    let mut pkt = Vec::with_capacity(total);
    pkt.extend_from_slice(&[0x45, 0x00]); // v4, IHL 5, ToS 0
    pkt.extend_from_slice(&(total as u16).to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ID
    pkt.extend_from_slice(&0u16.to_be_bytes()); // flags / fragment
    pkt.push(64); // TTL
    pkt.push(17); // UDP
    pkt.extend_from_slice(&[0, 0]); // header checksum placeholder
    pkt.extend_from_slice(&src.to_be_bytes());
    pkt.extend_from_slice(&dst.to_be_bytes());
    pkt.extend_from_slice(&sport.to_be_bytes());
    pkt.extend_from_slice(&dport.to_be_bytes());
    pkt.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes()); // UDP checksum 0
    pkt.extend_from_slice(payload);
    fix_ipv4_checksum(&mut pkt[..20]);
    pkt
}

/// Resolve `name` through a protected UDP socket (physical network), so the
/// query does not loop back into the TUN. One-shot socket per query: DNS is
/// low-rate and this keeps the code free of shared-socket state.
async fn resolve_upstream(name: &str, upstream: std::net::SocketAddr) -> Option<(Vec<u32>, u32)> {
    let sock = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )
    .ok()?;
    if !protect_fd(sock.as_raw_fd()) {
        log_e!("protect_fd failed for the DNS socket");
    }
    sock.set_nonblocking(true).ok()?;
    let sock = tokio::net::UdpSocket::from_std(sock.into()).ok()?;
    let query = dns::build_upstream_query(rand::random::<u16>(), name);
    tokio::time::timeout(Duration::from_secs(3), async move {
        sock.send_to(&query, upstream).await.ok()?;
        let mut buf = vec![0u8; 1500];
        let (n, _) = sock.recv_from(&mut buf).await.ok()?;
        dns::parse_upstream_response(&buf[..n], name)
    })
    .await
    .ok()
    .flatten()
}

/// Answer one intercepted DNS query: zone-matching A queries get a fake IP,
/// everything else is relayed with the real answer.
async fn handle_dns_query(
    engine: &Arc<tokio::sync::Mutex<DnsEngine>>,
    tun_tx: &mpsc::Sender<Vec<u8>>,
    client_ip: u32,
    client_port: u16,
    payload: Vec<u8>,
) {
    let Some(q) = dns::parse_query(&payload) else {
        return;
    };
    let (server_ip, upstream, zone_hit) = {
        let e = engine.lock().await;
        let hit = e.zones.iter().any(|z| dns::in_zone(&q.name, z));
        (e.server_ip, e.upstream, hit)
    };

    // Only A queries are meaningful here. AAAA/HTTPS-type queries get a
    // NOERROR-with-no-records answer so the resolver falls back to A fast.
    if q.qtype != 1 {
        let resp = dns::build_response(&q, &[], 0);
        let pkt = build_udp_packet(server_ip, client_ip, 53, client_port, &resp);
        let _ = tun_tx.send(pkt).await;
        return;
    }

    let Some((ips, ttl)) = resolve_upstream(&q.name, upstream).await else {
        return; // drop: the resolver retries or falls back on its own
    };

    let (answer, ttl) = if zone_hit {
        // Map the first resolved address to its fake IP; cap the TTL so the
        // client keeps re-asking and the pool stays fresh.
        let real = ips[0];
        let fake = engine.lock().await.pool.fake_for_real(real);
        (vec![fake], ttl.min(30))
    } else {
        (ips, ttl)
    };
    log_d!("DNS {} → {} ({})", q.name,
        answer.iter().map(|i| std::net::Ipv4Addr::from(*i).to_string())
            .collect::<Vec<_>>().join(","),
        if zone_hit { "fake" } else { "real" });

    let resp = dns::build_response(&q, &answer, ttl);
    let pkt = build_udp_packet(server_ip, client_ip, 53, client_port, &resp);
    let _ = tun_tx.send(pkt).await;
}

// ── Union mode: per-flow split tunneling (selected apps + selected sites) ────
//
// When Proxy split mode has BOTH apps and sites/zones selected, the TUN is
// opened with 0.0.0.0/0 and no app filter (see ForgeFoxVpnService), so every
// app's traffic enters the TUN. This engine routes each flow:
//   dst inside a selected site range  → SSH tunnel;
//   dst is a fake zone IP             → SSH tunnel (handled by DnsEngine NAT);
//   connection owner is a selected app→ SSH tunnel;
//   otherwise                         → bypass: re-originate the flow through
//                                       a protected socket outside the VPN.

/// Shared per-run union state: cached flow decisions + channels to the
/// per-flow bypass tasks.
struct UnionInner {
    table: FlowTable,
    channels: HashMap<FlowKey, mpsc::Sender<Vec<u8>>>,
}

struct Union {
    /// Selected site ranges (proxy_ranges in the settings JSON).
    ranges: RangeSet,
    state: tokio::sync::Mutex<UnionInner>,
}

impl Union {
    fn from_settings(settings: &Value) -> Option<Arc<Self>> {
        if !settings["union_mode"].as_bool().unwrap_or(false) {
            return None;
        }
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        if let Some(arr) = settings["proxy_ranges"].as_array() {
            for pair in arr {
                let Some(pair) = pair.as_array() else { continue };
                if pair.len() != 2 {
                    continue;
                }
                let (Ok(a), Ok(b)) = (
                    pair[0].as_str().unwrap_or("").parse::<std::net::Ipv4Addr>(),
                    pair[1].as_str().unwrap_or("").parse::<std::net::Ipv4Addr>(),
                ) else {
                    continue;
                };
                pairs.push((u32::from(a), u32::from(b)));
            }
        }
        log_d!(
            "Union mode ON: {} site ranges, flows decided per-connection",
            RangeSet::from_pairs(pairs.clone()).len()
        );
        Some(Arc::new(Self {
            ranges: RangeSet::from_pairs(pairs),
            state: tokio::sync::Mutex::new(UnionInner {
                table: FlowTable::new(8192),
                channels: HashMap::new(),
            }),
        }))
    }

    /// Drop a finished flow (called by bypass tasks on teardown).
    async fn remove_flow(&self, key: &FlowKey) {
        let mut st = self.state.lock().await;
        st.table.remove(key);
        st.channels.remove(key);
    }
}

/// UDP payload of an IPv4 packet (None if truncated).
fn udp_payload(pkt: &[u8]) -> Option<&[u8]> {
    if pkt.len() < 28 || pkt[0] >> 4 != 4 {
        return None;
    }
    let ihl = ((pkt[0] & 0x0F) as usize) * 4;
    if pkt.len() < ihl + 8 {
        return None;
    }
    Some(&pkt[ihl + 8..])
}

/// Route one outbound TUN packet. Returns true when it should be forwarded
/// through the SSH tunnel; false when the packet was consumed (bypassed,
/// dropped or handed to a per-flow task). Must run AFTER DnsEngine so DNS
/// interception and fake→real NAT take precedence. `zone_dst` tells whether
/// the packet's ORIGINAL destination (before the NAT) was a fake zone IP:
/// zone rules apply to every app, so such flows always tunnel.
async fn union_dispatch(
    u: &Arc<Union>,
    pkt: &[u8],
    tun_tx: mpsc::Sender<Vec<u8>>,
    zone_dst: bool,
) -> bool {
    // Fragments and other headerless packets cannot be attributed — the
    // first fragment of the same flow was already routed, so forward
    // (existing pre-union behavior).
    let Some(key) = flow_rules::flow_key_of(pkt) else {
        return true;
    };
    let (proto, src, sport, dst, dport) = key;

    // Follow a cached decision.
    {
        let st = u.state.lock().await;
        match st.table.route_for(&key) {
            Some(Route::Tunnel) => return true,
            Some(Route::Bypass) => {
                if let Some(ch) = st.channels.get(&key) {
                    // TCP tasks want whole packets, UDP tasks want payloads.
                    let data: Vec<u8> = if proto == PROTO_UDP {
                        match udp_payload(pkt) {
                            Some(p) => p.to_vec(),
                            None => return false,
                        }
                    } else {
                        pkt.to_vec()
                    };
                    if ch.try_send(data).is_ok() {
                        return false;
                    }
                    // Channel full: the flow's task is stuck or gone. For a
                    // mid-connection TCP packet there is nothing sane to do;
                    // for a SYN we can rebuild the connection below.
                }
                if proto == PROTO_TCP {
                    match tcp_gen::parse_tcp(pkt) {
                        Some(seg) if seg.flags & tcp_gen::SYN != 0 => { /* rebuild */ }
                        _ => return false, // stray post-teardown packet → drop
                    }
                }
            }
            None => {}
        }
    }

    // New flow: site ranges and zone fake IPs win for every app.
    if zone_dst || u.ranges.contains(dst) {
        u.state.lock().await.table.set(key, Route::Tunnel);
        return true;
    }

    // Ask Java who owns the connection; selected apps get the tunnel.
    let selected = match lookup_owner_selected(proto, src, sport, dst, dport).await {
        Some(true) => true,
        Some(false) => false,
        None => {
            // Owner unknown — tunnel (traffic still flows, just not split).
            u.state.lock().await.table.set(key, Route::Tunnel);
            return true;
        }
    };
    if selected {
        log_d!(
            "TUNNEL flow (selected app): {}:{} → {}",
            std::net::Ipv4Addr::from(src),
            sport,
            std::net::Ipv4Addr::from(dst)
        );
        u.state.lock().await.table.set(key, Route::Tunnel);
        return true;
    }

    // Bypass.
    match proto {
        PROTO_TCP => {
            let Some(seg) = tcp_gen::parse_tcp(pkt) else { return true };
            if seg.flags & tcp_gen::SYN == 0 {
                return false; // mid-connection packet with no state → drop
            }
            let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
            {
                let mut st = u.state.lock().await;
                st.table.set(key, Route::Bypass);
                st.channels.insert(key, tx);
            }
            tokio::spawn(tcp_bypass_conn(
                Arc::clone(u),
                key,
                pkt.to_vec(),
                rx,
                tun_tx,
            ));
            false
        }
        PROTO_UDP => {
            let Some(payload) = udp_payload(pkt) else { return false };
            let first = payload.to_vec();
            let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
            {
                let mut st = u.state.lock().await;
                st.table.set(key, Route::Bypass);
                st.channels.insert(key, tx);
            }
            tokio::spawn(udp_bypass_flow(Arc::clone(u), key, rx, tun_tx));
            // Deliver the first payload through the channel so the task owns
            // ordering from the start.
            if let Some(ch) = u.state.lock().await.channels.get(&key) {
                let _ = ch.try_send(first);
            }
            false
        }
        _ => true,
    }
}

/// Create a protected (VPN-exempt) UDP socket ready for tokio.
fn protected_udp_socket() -> Option<tokio::net::UdpSocket> {
    let sock = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )
    .ok()?;
    if !protect_fd(sock.as_raw_fd()) {
        log_e!("protect_fd failed for bypass UDP socket");
    }
    sock.set_nonblocking(true).ok()?;
    let std_sock: std::net::UdpSocket = sock.into();
    tokio::net::UdpSocket::from_std(std_sock).ok()
}

/// Connect a protected (VPN-exempt) TCP socket to the real destination.
/// The socket is protected BEFORE connecting so the SYN never enters the
/// TUN; the blocking connect runs on a worker thread.
async fn protected_tcp_connect(srv_ip: u32, srv_port: u16) -> Option<tokio::net::TcpStream> {
    let sock = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .ok()?;
    if !protect_fd(sock.as_raw_fd()) {
        log_e!("protect_fd failed for bypass TCP socket");
    }
    let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::from(srv_ip), srv_port));
    let std_sock: std::net::TcpStream = tokio::task::spawn_blocking(move || {
        let addr = socket2::SockAddr::from(addr);
        sock.connect_timeout(&addr, Duration::from_secs(10)).ok()?;
        sock.set_nonblocking(true).ok()?;
        let s: std::net::TcpStream = sock.into();
        Some(s)
    })
    .await
    .ok()??;
    match tokio::net::TcpStream::from_std(std_sock) {
        Ok(s) => Some(s),
        Err(e) => {
            log_d!(
                "bypass TCP connect {}:{} failed: {}",
                std::net::Ipv4Addr::from(srv_ip),
                srv_port,
                e
            );
            None
        }
    }
}

/// One bypassed UDP flow: relay payloads between the app (via TUN, through
/// the flow channel) and the real destination through a protected socket.
/// The app keeps talking to the original dst address: inbound datagrams are
/// re-crafted as if they came from it.
async fn udp_bypass_flow(
    u: Arc<Union>,
    key: FlowKey,
    mut rx: mpsc::Receiver<Vec<u8>>,
    tun_tx: mpsc::Sender<Vec<u8>>,
) {
    let (_proto, cli_ip, cli_port, srv_ip, srv_port) = key;
    let Some(sock) = protected_udp_socket() else {
        u.remove_flow(&key).await;
        return;
    };
    let dst = std::net::SocketAddr::from((std::net::Ipv4Addr::from(srv_ip), srv_port));
    let mut buf = vec![0u8; 65535];
    let mut idle = std::time::Instant::now();

    loop {
        let deadline = tokio::time::Instant::from_std(idle + Duration::from_secs(60));
        tokio::select! {
            maybe = rx.recv() => {
                match maybe {
                    None => break,
                    Some(payload) => {
                        let _ = sock.send_to(&payload, dst).await;
                        idle = std::time::Instant::now();
                    }
                }
            }
            r = sock.recv_from(&mut buf) => {
                match r {
                    Ok((n, peer)) => {
                        if peer.ip() == std::net::IpAddr::V4(std::net::Ipv4Addr::from(srv_ip)) {
                            let pkt = build_udp_packet(srv_ip, cli_ip, srv_port, cli_port, &buf[..n]);
                            if tun_tx.send(pkt).await.is_err() {
                                break;
                            }
                        }
                        idle = std::time::Instant::now();
                    }
                    Err(_) => break,
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                log_d!("bypass UDP flow idle-expired: {} → {}", cli_port, dst);
                break;
            }
        }
    }
    u.remove_flow(&key).await;
}

/// Move bytes from the pending server→client buffer into TCP segments,
/// respecting the client's advertised window. Returns the crafted packets.
#[allow(clippy::too_many_arguments)]
fn take_data_segments(
    pending: &mut VecDeque<u8>,
    retrans: &mut VecDeque<(u32, Vec<u8>)>,
    our_seq: &mut u32,
    ack: u32,
    in_flight: u32,
    window: u32,
    mss: usize,
    srv_ip: u32,
    srv_port: u16,
    cli_ip: u32,
    cli_port: u16,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut usable = window.saturating_sub(in_flight);
    while !pending.is_empty() && usable > 0 {
        let n = pending.len().min(usable as usize).min(mss);
        if n == 0 {
            break;
        }
        let payload: Vec<u8> = pending.drain(..n).collect();
        let pkt = tcp_gen::build_tcp_packet(
            srv_ip,
            cli_ip,
            srv_port,
            cli_port,
            *our_seq,
            ack,
            tcp_gen::PSH | tcp_gen::ACK,
            0xFFFF,
            &payload,
        );
        retrans.push_back((*our_seq, payload));
        *our_seq = our_seq.wrapping_add(n as u32);
        usable = usable.saturating_sub(n as u32);
        out.push(pkt);
    }
    out
}

/// A bypassed TCP connection. The core terminates TCP with the app (acting
/// as the remote endpoint) and relays the stream through a protected socket
/// to the real destination, outside the VPN.
async fn tcp_bypass_conn(
    u: Arc<Union>,
    key: FlowKey,
    syn: Vec<u8>,
    mut rx: mpsc::Receiver<Vec<u8>>,
    tun_tx: mpsc::Sender<Vec<u8>>,
) {
    let (_proto, cli_ip, cli_port, srv_ip, srv_port) = key;

    let Some(seg) = tcp_gen::parse_tcp(&syn) else {
        u.remove_flow(&key).await;
        return;
    };
    let cli_isn = seg.seq;
    let their_wscale = seg.wscale.unwrap_or(0);
    let mss = seg.mss.unwrap_or(536).min(tcp_gen::OUR_MSS) as usize;
    let our_isn: u32 = rand::random();

    // Connection state (app side).
    let mut expected = cli_isn.wrapping_add(1); // next byte we expect from the app
    let mut our_seq = our_isn.wrapping_add(1); // next byte we will send
    let mut snd_una = our_isn.wrapping_add(1); // oldest unacked byte of ours
    let mut their_window: u32 = (seg.window as u32) << their_wscale;
    let mut established = false;
    let mut client_fin = false;
    let mut server_eof = false; // real server closed its write side
    let mut fin_sent = false;

    // Server→client data path.
    let mut pending: VecDeque<u8> = VecDeque::new();
    let mut retrans: VecDeque<(u32, Vec<u8>)> = VecDeque::new();
    let mut rto = Duration::from_millis(1000);
    let mut last_xmit = std::time::Instant::now();
    let mut retries = 0u32;
    let mut last_activity = std::time::Instant::now();

    // Protected connection to the real destination.
    let stream = protected_tcp_connect(srv_ip, srv_port).await;
    let (mut s_read, mut s_write) = match stream {
        Some(s) => {
            let (r, w) = s.into_split();
            (Some(r), Some(w))
        }
        None => {
            log_d!(
                "bypass TCP: no direct route to {}:{}, refusing",
                std::net::Ipv4Addr::from(srv_ip),
                srv_port
            );
            let rst = tcp_gen::build_tcp_packet(
                srv_ip, cli_ip, srv_port, cli_port,
                our_isn, expected, tcp_gen::RST | tcp_gen::ACK, 0, &[],
            );
            let _ = tun_tx.send(rst).await;
            u.remove_flow(&key).await;
            return;
        }
    };

    // SYN-ACK: complete the handshake on behalf of the real destination.
    let synack = tcp_gen::build_tcp_packet(
        srv_ip, cli_ip, srv_port, cli_port,
        our_isn, expected, tcp_gen::SYN | tcp_gen::ACK, 0xFFFF, &[],
    );
    if tun_tx.send(synack).await.is_err() {
        u.remove_flow(&key).await;
        return;
    }

    let mut rbuf = vec![0u8; 65536];
    log_d!(
        "bypass TCP flow: {}:{} → {}:{}",
        std::net::Ipv4Addr::from(cli_ip),
        cli_port,
        std::net::Ipv4Addr::from(srv_ip),
        srv_port
    );

    // Drain pending server data into segments (window permitting). Returns
    // the crafted packets; updates our_seq/retrans/last_xmit.
    macro_rules! flush_pending {
        () => {{
            let in_flight = our_seq.wrapping_sub(snd_una);
            let pkts = take_data_segments(
                &mut pending, &mut retrans, &mut our_seq, expected, in_flight,
                their_window, mss, srv_ip, srv_port, cli_ip, cli_port,
            );
            let sent = !pkts.is_empty();
            for p in pkts {
                let _ = tun_tx.send(p).await;
            }
            if sent {
                last_xmit = std::time::Instant::now();
            }
        }};
    }

    // Send our FIN once all server data is out and acked.
    macro_rules! maybe_send_fin {
        () => {
            if server_eof && !fin_sent && pending.is_empty() && retrans.is_empty() {
                let fin = tcp_gen::build_tcp_packet(
                    srv_ip, cli_ip, srv_port, cli_port,
                    our_seq, expected, tcp_gen::FIN | tcp_gen::ACK, 0xFFFF, &[],
                );
                let _ = tun_tx.send(fin).await;
                our_seq = our_seq.wrapping_add(1);
                fin_sent = true;
                last_xmit = std::time::Instant::now();
            }
        };
    }

    loop {
        // 4 Hz maintenance tick: retransmits, deferred FIN, idle reap.
        let mut ticker = tokio::time::interval(Duration::from_millis(250));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tokio::select! {
            maybe_pkt = rx.recv() => {
                let Some(pkt) = maybe_pkt else { break };
                last_activity = std::time::Instant::now();
                let Some(seg) = tcp_gen::parse_tcp(&pkt) else { continue };
                if seg.flags & tcp_gen::RST != 0 {
                    break;
                }
                if seg.flags & tcp_gen::SYN != 0 {
                    // Handshake retransmit before our SYN-ACK arrived.
                    if !established && seg.seq == cli_isn {
                        let p = tcp_gen::build_tcp_packet(
                            srv_ip, cli_ip, srv_port, cli_port,
                            our_isn, expected, tcp_gen::SYN | tcp_gen::ACK, 0xFFFF, &[],
                        );
                        let _ = tun_tx.send(p).await;
                    }
                    continue;
                }
                if seg.flags & tcp_gen::ACK != 0 {
                    if tcp_gen::seq_lt(snd_una, seg.ack) {
                        snd_una = seg.ack;
                        while let Some((seq, data)) = retrans.front() {
                            if tcp_gen::seq_le(seq.wrapping_add(data.len() as u32), snd_una) {
                                retrans.pop_front();
                            } else {
                                break;
                            }
                        }
                        if retrans.is_empty() {
                            retries = 0;
                            rto = Duration::from_millis(1000);
                        }
                    }
                    their_window = (seg.window as u32) << their_wscale;
                    if !established && seg.ack == our_isn.wrapping_add(1) {
                        established = true;
                    }
                }
                let data = seg.payload(&pkt);
                if !data.is_empty() {
                    if seg.seq == expected {
                        if let Some(w) = s_write.as_mut() {
                            if w.write_all(data).await.is_err() {
                                break;
                            }
                        }
                        expected = expected.wrapping_add(data.len() as u32);
                    } else if tcp_gen::seq_lt(seg.seq, expected) {
                        // retransmit of data we already have → just re-ack
                    } else {
                        continue; // out of order → drop, wait for retransmit
                    }
                    let ack_pkt = tcp_gen::build_tcp_packet(
                        srv_ip, cli_ip, srv_port, cli_port,
                        our_seq, expected, tcp_gen::ACK, 0xFFFF, &[],
                    );
                    let _ = tun_tx.send(ack_pkt).await;
                }
                if seg.flags & tcp_gen::FIN != 0 && !client_fin {
                    if seg.seq.wrapping_add(data.len() as u32) == expected {
                        expected = expected.wrapping_add(1);
                        client_fin = true;
                        let ack_pkt = tcp_gen::build_tcp_packet(
                            srv_ip, cli_ip, srv_port, cli_port,
                            our_seq, expected, tcp_gen::ACK, 0xFFFF, &[],
                        );
                        let _ = tun_tx.send(ack_pkt).await;
                        if let Some(w) = s_write.as_mut() {
                            let _ = w.shutdown().await;
                        }
                    }
                }
                // An ACK may have opened the send window.
                flush_pending!();
                maybe_send_fin!();
                // Fully closed in both directions and everything acked?
                if client_fin && fin_sent && retrans.is_empty() && snd_una == our_seq {
                    break;
                }
            }
            r = async {
                match s_read.as_mut() {
                    Some(r) => r.read(&mut rbuf).await,
                    None => std::future::pending::<io::Result<usize>>().await,
                }
            }, if s_read.is_some() => {
                match r {
                    Ok(0) => {
                        // Real server closed: deliver FIN after pending data.
                        s_read = None;
                        server_eof = true;
                        flush_pending!();
                        maybe_send_fin!();
                        if client_fin && fin_sent && retrans.is_empty() && snd_una == our_seq {
                            break;
                        }
                    }
                    Ok(n) => {
                        pending.extend(rbuf[..n].iter().copied());
                        flush_pending!();
                    }
                    Err(_) => break,
                }
            }
            _ = ticker.tick() => {
                // Retransmit our unacked data.
                if !retrans.is_empty() && last_xmit.elapsed() > rto {
                    if let Some((seq, payload)) = retrans.front().cloned() {
                        let p = tcp_gen::build_tcp_packet(
                            srv_ip, cli_ip, srv_port, cli_port,
                            seq, expected, tcp_gen::PSH | tcp_gen::ACK, 0xFFFF, &payload,
                        );
                        let _ = tun_tx.send(p).await;
                        retries += 1;
                        rto = (rto * 2).min(Duration::from_secs(8));
                        last_xmit = std::time::Instant::now();
                        if retries > 6 {
                            let rst = tcp_gen::build_tcp_packet(
                                srv_ip, cli_ip, srv_port, cli_port,
                                our_seq, expected, tcp_gen::RST | tcp_gen::ACK, 0, &[],
                            );
                            let _ = tun_tx.send(rst).await;
                            break;
                        }
                    }
                }
                maybe_send_fin!();
                // Idle timeout: nothing from either side for a long while.
                if last_activity.elapsed() > Duration::from_secs(300) {
                    log_d!("bypass TCP flow idle-expired");
                    break;
                }
            }
        }
    }

    u.remove_flow(&key).await;
}

#[no_mangle]
pub extern "system" fn Java_com_forgefox_vpn_Core_startSshVpn(
    mut env: JNIEnv,
    _class: JClass,
    fd: jint,
    settings_json_str: JString,
) {
    let settings_str: String = match env.get_string(&settings_json_str) {
        Ok(s) => s.into(),
        Err(_) => {
            println!("Failed to get settings JSON string");
            return;
        }
    };
    let settings: Value = serde_json::from_str(&settings_str).unwrap_or(serde_json::json!({}));

    // Cache the JVM and a global ref of Core so background tasks can call
    // protect() on sockets created later (upstream DNS).
    if let Ok(vm) = env.get_java_vm() {
        if let Ok(mut guard) = JAVA_VM.lock() {
            *guard = Some(vm);
        }
    }
    if let Ok(class) = env.find_class("com/forgefox/vpn/Core") {
        if let Ok(global) = env.new_global_ref(class) {
            if let Ok(mut guard) = CORE_CLASS.lock() {
                *guard = Some(global);
            }
        }
    }

    // Read connection parameters
    let host = settings["host"].as_str().unwrap_or("127.0.0.1").to_string();
    let port = settings["port"].as_u64().unwrap_or(22);
    let user = settings["user"].as_str().unwrap_or("root").to_string();
    let pass = settings["pass"].as_str().unwrap_or("").to_string();
    let server_tun_ip = settings["server_tun_ip"].as_str().unwrap_or("10.0.0.1").to_string();
    // Must match the MTU the Kotlin side sets on the local TUN (1400), otherwise
    // the server-side adapter stays at 1500 and packets fragment.
    let mtu = settings["mtu"].as_u64().unwrap_or(1400) as u16;

    let host_port = format!("{}:{}", host, port);
    let addrs: Vec<std::net::SocketAddr> = match std::net::ToSocketAddrs::to_socket_addrs(&host_port) {
        Ok(iter) => iter.collect(),
        Err(e) => {
            println!("DNS error: {:?}", e);
            return;
        }
    };
    if addrs.is_empty() {
        println!("No addresses resolved");
        return;
    }
    let addr = addrs[0];

    let domain = match addr {
        std::net::SocketAddr::V4(_) => socket2::Domain::IPV4,
        std::net::SocketAddr::V6(_) => socket2::Domain::IPV6,
    };
    let socket = match socket2::Socket::new(domain, socket2::Type::STREAM, None) {
        Ok(s) => s,
        Err(e) => {
            println!("Socket create error: {:?}", e);
            return;
        }
    };
    let sock_fd = socket.as_raw_fd();

    // Protect socket using JNI (sync)
    let _ = env.call_static_method(_class, "protectFd", "(I)Z", &[jni::objects::JValue::Int(sock_fd)]);

    // Connect socket
    if let Err(e) = socket.connect(&socket2::SockAddr::from(addr)) {
        println!("Connect error: {:?}", e);
        return;
    }
    if let Err(e) = socket.set_nonblocking(true) {
        println!("set_nonblocking error: {:?}", e);
        return;
    }
    let std_socket: std::net::TcpStream = socket.into();

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            println!("Tokio runtime error: {:?}", e);
            return;
        }
    };

    let (tx, mut rx) = tokio::sync::broadcast::channel(1);
    if let Ok(mut lock) = CANCEL_TX.lock() {
        *lock = Some(tx);
    }

    rt.block_on(async move {
        tokio::select! {
            _ = rx.recv() => {
                log_d!("VPN cancelled by user.");
            }
            _ = async {
                // Create Tokio stream INSIDE the runtime
        let tokio_stream = match tokio::net::TcpStream::from_std(std_socket) {
            Ok(s) => s,
            Err(e) => {
                println!("tokio from_std error: {:?}", e);
                return;
            }
        };
        let config = russh::client::Config::default();
        let config = Arc::new(config);

        log_d!("Connecting to SSH...");
        let mut session = match russh::client::connect_stream(config, tokio_stream, ClientHandler).await {
            Ok(s) => {
                log_d!("SSH connected successfully");
                s
            },
            Err(e) => {
                log_e!("SSH connect failed: {}", e);
                return;
            }
        };

        log_d!("Authenticating with password...");
        let auth_res = session.authenticate_password(user, pass).await;
        match auth_res {
            Ok(true) => log_d!("Password auth success"),
            Ok(false) => {
                log_e!("Password auth failed (rejected)");
                return;
            }
            Err(e) => {
                log_e!("Password auth error: {}", e);
                return;
            }
        }

        log_d!("Opening session channel...");
        if let Ok(mut channel) = session.channel_open_session().await {
            log_d!("Channel opened, sending server bridge command...");
            // Same server-side command as the desktop client, so Android speaks
            // the protocol of hosts installed by the desktop app:
            //   - fast path: forgefox-bridge (installed by install.sh) — the
            //     restricted ff-shell on those hosts only allows commands
            //     containing "forgefox-bridge" or "python", and this one has both;
            //   - fallback: inline Python that also raises the TUN, enables
            //     forwarding and sets up NAT, so bare servers work too.
            let python_script = format!("python3 -u -c \"
import os,struct,fcntl,sys,select
try:
 tun=open('/dev/net/tun','r+b',buffering=0)
 req=bytearray(b'\\x00'*16+struct.pack('H',0x1001))
 fcntl.ioctl(tun,0x400454ca,req)
 i=req[:16].strip(b'\\x00').decode('ascii')
 os.system('ip link set '+i+' up && ip addr replace {0}/24 dev '+i+' && ip link set dev '+i+' mtu {1}')
 net='.'.join('{0}'.split('.')[:3])+'.0/24'
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
 pass
\"", server_tun_ip, mtu);

            let bridge_cmd = format!(
                "if command -v forgefox-bridge >/dev/null 2>&1; then exec forgefox-bridge {} {}; else {}; fi",
                server_tun_ip, mtu, python_script
            );

            match channel.exec(true, bridge_cmd).await {
                Ok(_) => log_d!("Exec server bridge success"),
                Err(e) => log_e!("Exec server bridge failed: {}", e)
            }

            let stream = channel.into_stream();
            if let Ok(tun_fd) = TunFd::new(fd as RawFd) {
                log_d!("Starting bidirectional bridged loop...");
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let engine = Arc::new(tokio::sync::Mutex::new(DnsEngine::from_settings(&settings)));
                let (mut tun_r, mut tun_w) = tokio::io::split(tun_fd);
                let (mut s_rx, mut s_tx) = tokio::io::split(stream);

                // Shared TUN writer: SSH-inbound packets and locally crafted
                // DNS responses both land here.
                let (tun_w_tx, mut tun_w_rx) = mpsc::channel::<Vec<u8>>(256);
                tokio::spawn(async move {
                    while let Some(pkt) = tun_w_rx.recv().await {
                        if tun_w.write_all(&pkt).await.is_err() {
                            log_e!("Failed to write packet to TUN");
                            break;
                        }
                    }
                });

                // DNS interceptor task: resolves queries and sends crafted
                // responses to the TUN writer.
                let dns_engine = Arc::clone(&engine);
                let dns_tun_tx = tun_w_tx.clone();
                let (dns_tx, mut dns_rx) = mpsc::channel::<(Vec<u8>, u32, u16)>(64);
                tokio::spawn(async move {
                    while let Some((payload, cip, cport)) = dns_rx.recv().await {
                        handle_dns_query(&dns_engine, &dns_tun_tx, cip, cport, payload).await;
                    }
                });

                // TUN → SSH: DNS interception + fake→real NAT + (in union
                // mode) per-flow tunnel/bypass decisions.
                let tx_engine = Arc::clone(&engine);
                let union = Union::from_settings(&settings);
                let tx_tun_tx = tun_w_tx.clone();
                let tx_task = tokio::spawn(async move {
                    let mut buf = vec![0u8; 65535];
                    loop {
                        match tun_r.read(&mut buf).await {
                            Ok(0) => { log_d!("Tun read EOF"); break; }
                            Ok(n) => {
                                // Destination BEFORE the DNS engine's fake→real
                                // NAT: a fake dst means the domain matched a
                                // selected zone — in union mode that wins over
                                // the owner check for every app.
                                let zone_dst = n >= 20 && {
                                    let orig_dst = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
                                    dns::is_fake_ip(orig_dst)
                                };
                                match tx_engine.lock().await.process_outbound(&mut buf[..n]) {
                                    OutAction::Drop => continue,
                                    OutAction::Dns(payload, cip, cport) => {
                                        if dns_tx.send((payload, cip, cport)).await.is_err() { break; }
                                    }
                                    OutAction::Forward => {
                                        let to_tunnel = match &union {
                                            Some(u) => {
                                                let pkt = buf[..n].to_vec();
                                                union_dispatch(u, &pkt, tx_tun_tx.clone(), zone_dst).await
                                            }
                                            None => true,
                                        };
                                        if to_tunnel {
                                            let len_bytes = (n as u16).to_be_bytes();
                                            if s_tx.write_all(&len_bytes).await.is_err() { log_e!("Failed to write len to stream"); break; }
                                            if s_tx.write_all(&buf[..n]).await.is_err() { log_e!("Failed to write pkt to stream"); break; }
                                            if s_tx.flush().await.is_err() { log_e!("Failed to flush stream"); break; }
                                        }
                                    }
                                }
                            }
                            Err(e) => { log_e!("Tun read error: {}", e); break; },
                        }
                    }
                });

                // SSH → TUN: real→fake NAT.
                let rx_engine = Arc::clone(&engine);
                let rx_task = tokio::spawn(async move {
                    let mut len_buf = [0u8; 2];
                    let mut pkt_buf = vec![0u8; 65536];
                    loop {
                        if s_rx.read_exact(&mut len_buf).await.is_err() { log_e!("Failed to read len from stream"); break; }
                        let len = u16::from_be_bytes(len_buf) as usize;
                        if len == 0 { continue; } // zero-length frame: skip, don't spin
                        if s_rx.read_exact(&mut pkt_buf[..len]).await.is_err() { log_e!("Failed to read pkt from stream"); break; }
                        rx_engine.lock().await.process_inbound(&mut pkt_buf[..len]);
                        if tun_w_tx.send(pkt_buf[..len].to_vec()).await.is_err() { break; }
                    }
                });

                let _ = tokio::try_join!(tx_task, rx_task);
                // The runtime shuts down with the bridge: pending DNS
                // queries and the TUN writer die with it, no drain needed.
                log_d!("Bidirectional loop finished");
            } else {
                log_e!("Failed to create TunFd");
            }
        } else {
            log_e!("Failed to open session channel");
        }
        log_d!("SSH session ended");
            } => {}
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_forgefox_vpn_Core_stopSshVpn(
    _env: JNIEnv,
    _class: JClass,
) {
    if let Ok(mut lock) = CANCEL_TX.lock() {
        if let Some(tx) = lock.take() {
            let _ = tx.send(());
            log_d!("Cancellation signal sent");
        }
    }
}
