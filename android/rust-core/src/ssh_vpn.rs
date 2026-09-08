use jni::objects::{JClass, JString};
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

static CANCEL_TX: Mutex<Option<tokio::sync::broadcast::Sender<()>>> = Mutex::new(None);

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
                    msg.as_ptr()
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
                    msg.as_ptr()
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
                let (mut tun_rx, mut tun_tx) = tokio::io::split(tun_fd);
                let (mut stream_rx, mut stream_tx) = tokio::io::split(stream);

                let tx_task = tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    loop {
                        match tun_rx.read(&mut buf).await {
                            Ok(0) => { log_d!("Tun read EOF"); break; },
                            Ok(n) => {
                                let len_bytes = (n as u16).to_be_bytes();
                                if stream_tx.write_all(&len_bytes).await.is_err() { log_e!("Failed to write len to stream"); break; }
                                if stream_tx.write_all(&buf[..n]).await.is_err() { log_e!("Failed to write pkt to stream"); break; }
                                if stream_tx.flush().await.is_err() { log_e!("Failed to flush stream"); break; }
                            }
                            Err(e) => { log_e!("Tun read error: {}", e); break; },
                        }
                    }
                });

                let rx_task = tokio::spawn(async move {
                    let mut len_buf = [0u8; 2];
                    let mut pkt_buf = vec![0u8; 65536];
                    loop {
                        if stream_rx.read_exact(&mut len_buf).await.is_err() { log_e!("Failed to read len from stream"); break; }
                        let len = u16::from_be_bytes(len_buf) as usize;
                        if len == 0 { continue; } // zero-length frame: skip, don't spin
                        if stream_rx.read_exact(&mut pkt_buf[..len]).await.is_err() { log_e!("Failed to read pkt from stream"); break; }
                        if tun_tx.write_all(&pkt_buf[..len]).await.is_err() { log_e!("Failed to write pkt to tun"); break; }
                    }
                });

                let _ = tokio::try_join!(tx_task, rx_task);
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
