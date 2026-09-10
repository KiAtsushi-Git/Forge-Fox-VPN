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
use std::sync::Mutex;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::dns;

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

                // TUN → SSH: DNS interception + fake→real NAT.
                let tx_engine = Arc::clone(&engine);
                let tx_task = tokio::spawn(async move {
                    let mut buf = vec![0u8; 65535];
                    loop {
                        match tun_r.read(&mut buf).await {
                            Ok(0) => { log_d!("Tun read EOF"); break; }
                            Ok(n) => {
                                match tx_engine.lock().await.process_outbound(&mut buf[..n]) {
                                    OutAction::Drop => continue,
                                    OutAction::Dns(payload, cip, cport) => {
                                        if dns_tx.send((payload, cip, cport)).await.is_err() { break; }
                                    }
                                    OutAction::Forward => {
                                        let len_bytes = (n as u16).to_be_bytes();
                                        if s_tx.write_all(&len_bytes).await.is_err() { log_e!("Failed to write len to stream"); break; }
                                        if s_tx.write_all(&buf[..n]).await.is_err() { log_e!("Failed to write pkt to stream"); break; }
                                        if s_tx.flush().await.is_err() { log_e!("Failed to flush stream"); break; }
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
