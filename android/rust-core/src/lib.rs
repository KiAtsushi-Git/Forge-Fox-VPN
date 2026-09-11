pub mod dns;
pub mod flow_rules;
pub mod tcp_gen;
// The SSH/TUN bridge is Android-only (JNI, Unix fds); the cfg gate lets
// `cargo test` on a dev host still run the pure modules' unit tests.
#[cfg(target_os = "android")]
pub mod ssh_vpn;
