use jni::objects::{GlobalRef, JClass, JString};
use jni::sys::{jboolean, jint};
use jni::JNIEnv;
use serde_json::Value;
use std::os::unix::io::{RawFd, AsRawFd};
use std::sync::Arc;
use russh::*;
use std::io;
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, ReadBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::sync::Mutex;
use async_trait::async_trait;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::time::Duration;
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc;

use crate::dns::{self, DnsAnswer};

static CANCEL_TX: Mutex<Option<tokio::sync::broadcast::Sender<()>>> = Mutex::new(None);

/// Pending TUN-fd swap request: the new fd plus a one-shot ack the caller
/// waits on so it knows the old fd is no longer in use (Kotlin closes it).
type SwapMsg = (RawFd, std_mpsc::Sender<()>);
static SWAP_TX: Mutex<Option<mpsc::Sender<SwapMsg>>> = Mutex::new(None);

/// JVM handle + a global reference to the Core class, captured on the JNI
/// entry thread so background tasks can call `Core.onDnsLearned` later.
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
                    Ok(res)
                }
            }) {
                Ok(res) => return Poll::Ready(res.map(|n| n as usize)),
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

// ── Split-tunnel DNS observation ──────────────────────────────────────────────

/// Domain / domain-zone rules the bridge should watch DNS answers for.
/// Mirrors the desktop engine: `exact` matches a whole hostname, `zones`
/// match the apex plus every subdomain. Learned addresses are reported to
/// Kotlin (`Core.onDnsLearned`) which re-establishes the TUN routes —
/// VpnService routes are fixed at establish() time, so that is the only
/// moment they can change without tearing the SSH session down.
struct DomainMatcher {
    enabled: bool,
    exact: Vec<String>,
    zones: Vec<String>,
}

impl DomainMatcher {
    fn from_settings(settings: &Value) -> Self {
        fn norm_list(v: &Value, key: &str) -> Vec<String> {
            v[key]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str())
                        .map(|s| {
                            s.trim()
                                .trim_end_matches('.')
                                .trim_start_matches("*.")
                                .to_ascii_lowercase()
                        })
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        }
        Self {
            enabled: settings["split_enabled"].as_bool().unwrap_or(false),
            exact: norm_list(settings, "split_domains"),
            zones: norm_list(settings, "split_zones"),
        }
    }

    /// Returns the freshly learned addresses if this DNS answer matches a
    /// domain/zone rule. Already-known addresses are skipped so the Kotlin
    /// callback fires only when the route table actually needs to change.
    fn observe(&self, answer: &DnsAnswer, learned: &mut HashSet<u32>) -> Option<(Vec<Ipv4Addr>, String)> {
        if !self.enabled || (self.exact.is_empty() && self.zones.is_empty()) {
            return None;
        }
        let matched = answer.names.iter().any(|n| {
            self.exact.iter().any(|d| n == d) || self.zones.iter().any(|z| dns::in_zone(n, z))
        });
        if !matched {
            return None;
        }
        let fresh: Vec<Ipv4Addr> = answer
            .ips
            .iter()
            .copied()
            .filter(|ip| learned.insert(u32::from(*ip)))
            .collect();
        if fresh.is_empty() {
            None
        } else {
            Some((fresh, answer.query.clone()))
        }
    }
}

/// Notify Kotlin about addresses learned from a DNS answer, so it can
/// re-establish the TUN with updated routes. Runs on a tokio worker thread:
/// attach to the JVM, call `Core.onDnsLearned(String)`, detach.
fn notify_learned(ips: &[Ipv4Addr], query: &str) {
    // JavaVM is not Clone in jni 0.21, so keep the mutex guards and work
    // through references for the duration of the call.
    let vm_lock = match JAVA_VM.lock() {
        Ok(l) => l,
        Err(_) => return,
    };
    let class_lock = match CORE_CLASS.lock() {
        Ok(l) => l,
        Err(_) => return,
    };
    let Some(vm) = vm_lock.as_ref() else { return };
    let Some(class_ref) = class_lock.as_ref() else { return };

    let mut env = match vm.attach_current_thread() {
        Ok(guard) => guard,
        Err(e) => {
            log_e!("JVM attach failed: {e:?}");
            return;
        }
    };

    let payload = serde_json::json!({
        "ips": ips.iter().map(|ip| ip.to_string()).collect::<Vec<_>>(),
        "query": query,
    });
    let jstr = match env.new_string(payload.to_string()) {
        Ok(s) => s,
        Err(e) => {
            log_e!("new_string failed: {e:?}");
            return;
        }
    };

    // Safety: the raw pointer is a JNI global reference owned by `class_ref`,
    // which stays alive for the duration of the call.
    let class = unsafe { JClass::from_raw(class_ref.as_raw() as _) };
    let res = env.call_static_method(
        class,
        "onDnsLearned",
        "(Ljava/lang/String;)V",
        &[jni::objects::JValue::Object(&jstr)],
    );
    if let Err(e) = res {
        log_e!("onDnsLearned callback failed: {e:?}");
    }
}

// ── Bridge loop with hot TUN swap ─────────────────────────────────────────────

/// Why the current bridge generation ended.
enum GenOutcome {
    /// Kotlin re-established the TUN: (new fd, ack sender).
    Swap(RawFd, std_mpsc::Sender<()>),
    /// The TUN interface died — often the old side of a re-establish, so
    /// the caller waits briefly for a swap before giving up.
    TunDown,
    /// The SSH stream or the whole VPN is gone.
    Done,
}

/// Bidirectional framed bridge between the local TUN and the SSH channel.
///
/// Unlike the desktop client, Android VpnService routes are decided once at
/// `establish()`; to apply newly learned split-tunnel addresses Kotlin
/// re-establishes the TUN (new fd) and hands it over via `Core.swapTunFd`.
/// The SSH session stays alive across swaps, so a new site learned from a
/// DNS answer does not cost a reconnect.
async fn run_bridge<S>(
    mut stream: S,
    initial_fd: RawFd,
    matcher: &DomainMatcher,
    swap_rx: &mut mpsc::Receiver<SwapMsg>,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut current_fd = initial_fd;
    let mut learned: HashSet<u32> = HashSet::new();
    // Stream bytes pending framing; survives fd swaps so a frame split
    // across a swap does not desynchronize the stream.
    let mut pending: Vec<u8> = Vec::with_capacity(65536);
    let mut tun_buf = [0u8; 4096];
    let mut s_buf = [0u8; 65536];

    'outer: loop {
        let mut tun = match TunFd::new(current_fd) {
            Ok(t) => t,
            Err(e) => {
                log_e!("TunFd::new failed: {e}");
                break 'outer;
            }
        };
        let (mut t_rx, mut t_tx) = tokio::io::split(&mut tun);
        let (mut s_rx, mut s_tx) = tokio::io::split(&mut stream);

        let outcome = loop {
            tokio::select! {
                r = t_rx.read(&mut tun_buf) => match r {
                    Ok(0) => break GenOutcome::TunDown,
                    Ok(n) => {
                        let len_bytes = (n as u16).to_be_bytes();
                        let ok = s_tx.write_all(&len_bytes).await.is_ok()
                            && s_tx.write_all(&tun_buf[..n]).await.is_ok()
                            && s_tx.flush().await.is_ok();
                        if !ok {
                            log_e!("Failed to write packet to SSH stream");
                            break GenOutcome::Done;
                        }
                        continue;
                    }
                    Err(_) => break GenOutcome::TunDown,
                },
                r = s_rx.read(&mut s_buf) => match r {
                    Ok(0) => break GenOutcome::Done,
                    Ok(n) => {
                        pending.extend_from_slice(&s_buf[..n]);
                        let mut tun_dead = false;
                        while pending.len() >= 2 {
                            let len = u16::from_be_bytes([pending[0], pending[1]]) as usize;
                            if len == 0 {
                                pending.drain(..2); // zero-length frame: skip
                                continue;
                            }
                            if pending.len() < 2 + len {
                                break; // partial frame
                            }
                            let frame: Vec<u8> = pending.drain(..2 + len).skip(2).collect();

                            // Inbound DNS answers keep domain / domain-zone
                            // rules current: CDNs hand out different addresses
                            // per lookup, so a one-shot resolve at connect
                            // time goes stale fast.
                            if let Some(answer) = dns::parse_ipv4_dns_packet(&frame) {
                                if let Some((ips, query)) = matcher.observe(&answer, &mut learned) {
                                    log_d!("Split: {} → {} (learned)", query, ips.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","));
                                    notify_learned(&ips, &query);
                                }
                            }

                            if t_tx.write_all(&frame).await.is_err() {
                                log_e!("Failed to write packet to TUN");
                                tun_dead = true;
                                break;
                            }
                        }
                        if tun_dead {
                            break GenOutcome::TunDown;
                        }
                        continue;
                    }
                    Err(_) => break GenOutcome::Done,
                },
                msg = swap_rx.recv() => match msg {
                    Some(msg) => break GenOutcome::Swap(msg.0, msg.1),
                    None => break GenOutcome::Done,
                },
            }
        };

        match outcome {
            GenOutcome::Swap(fd, ack) => {
                log_d!("TUN fd swapped: {} → {}", current_fd, fd);
                current_fd = fd;
                // Old fd is owned and closed by Kotlin once this ack arrives.
                let _ = ack.send(());
                continue 'outer;
            }
            GenOutcome::TunDown => {
                // Kotlin re-establishes right before handing over the new fd,
                // so the old interface often dies first — wait a moment.
                match tokio::time::timeout(Duration::from_secs(3), swap_rx.recv()).await {
                    Ok(Some((fd, ack))) => {
                        log_d!("TUN fd swapped after teardown: {} → {}", current_fd, fd);
                        current_fd = fd;
                        let _ = ack.send(());
                        continue 'outer;
                    }
                    _ => {
                        log_d!("TUN closed, no swap pending — ending bridge");
                        break 'outer;
                    }
                }
            }
            GenOutcome::Done => break 'outer,
        }
    }
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
    let matcher = DomainMatcher::from_settings(&settings);

    // Cache the JVM and a global ref of Core so background tasks can call
    // back into Kotlin (DNS-learned addresses).
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

    // Channel for hot TUN-fd swaps requested by Kotlin (route rebuilds).
    let (swap_tx, mut swap_rx) = mpsc::channel::<SwapMsg>(4);
    if let Ok(mut lock) = SWAP_TX.lock() {
        *lock = Some(swap_tx);
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
                if let Ok(channel) = session.channel_open_session().await {
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
                    log_d!("Starting bridged loop (with hot TUN swap)...");
                    run_bridge(stream, fd as RawFd, &matcher, &mut swap_rx).await;
                    log_d!("Bidirectional loop finished");
                } else {
                    log_e!("Failed to open session channel");
                }
                log_d!("SSH session ended");
            } => {}
        }
    });

    // Session over: reject further swap requests until the next connect.
    if let Ok(mut lock) = SWAP_TX.lock() {
        *lock = None;
    }
}

/// Hand a freshly established TUN fd to the running bridge (hot swap) and
/// wait for the bridge to stop using the old one. Returns true on success.
#[no_mangle]
pub extern "system" fn Java_com_forgefox_vpn_Core_swapTunFd(
    _env: JNIEnv,
    _class: JClass,
    fd: jint,
) -> jboolean {
    let tx = SWAP_TX.lock().ok().and_then(|g| g.clone());
    let Some(tx) = tx else {
        log_e!("swapTunFd: no running bridge");
        return 0;
    };

    let (ack_tx, ack_rx) = std_mpsc::channel::<()>();
    if tx.blocking_send((fd as RawFd, ack_tx)).is_err() {
        log_e!("swapTunFd: bridge gone");
        return 0;
    }
    // The bridge acks once the old fd is no longer used; Kotlin then closes it.
    match ack_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(()) => 1,
        Err(_) => {
            log_e!("swapTunFd: ack timeout");
            0
        }
    }
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
