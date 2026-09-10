pub mod dns;
// The SSH/TUN bridge is Android-only (JNI, Unix fds); the cfg gate lets
// `cargo test` on a dev host still run the dns module's unit tests.
#[cfg(target_os = "android")]
pub mod ssh_vpn;
