use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use russh::client;
use russh_keys::key::PublicKey;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use crate::config::SshServer;

struct ClientHandler;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;
    async fn check_server_key(&mut self, _server_public_key: &PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Connect and password-authenticate an SSH session to `server`.
async fn connect(server: &SshServer) -> Result<client::Handle<ClientHandler>> {
    use tokio::net::TcpStream;
    let addr = format!("{}:{}", server.host, server.port);
    let stream = tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect(&addr))
        .await
        .context("TCP connection timeout")?
        .context("TCP connect failed")?;

    let config = Arc::new(client::Config::default());
    let mut session = tokio::time::timeout(std::time::Duration::from_secs(5), client::connect_stream(config, stream, ClientHandler))
        .await
        .context("SSH handshake timeout")?
        .context("SSH handshake failed")?;

    let auth_ok = session.authenticate_password(server.username.clone(), server.password.clone()).await.context("SSH auth error")?;
    if !auth_ok { bail!("SSH password authentication rejected"); }
    Ok(session)
}

pub async fn run_remote_cmd(server: &SshServer, cmd: &str) -> Result<String> {
    let mut session = connect(server).await?;
    let channel = session.channel_open_session().await.context("channel_open_session failed")?;
    channel.exec(true, cmd.as_bytes()).await.context("Failed to exec cmd")?;

    let mut output = String::new();
    let stream = channel.into_stream();
    let mut rx = tokio::io::split(stream).0;
    rx.read_to_string(&mut output).await?;

    Ok(output)
}

/// Same as `run_remote_cmd`, but every complete output line is passed to
/// `on_line` the moment it arrives. Long installs (docker pull, a 5-15 min
/// source build) would otherwise be a black box until the very end — the
/// command output only used to surface when the exec channel closed.
///
/// Returns the full output, like `run_remote_cmd`.
pub async fn run_remote_cmd_streaming<F>(server: &SshServer, cmd: &str, mut on_line: F) -> Result<String>
where
    F: FnMut(&str),
{
    let mut session = connect(server).await?;
    let channel = session.channel_open_session().await.context("channel_open_session failed")?;
    channel.exec(true, cmd.as_bytes()).await.context("Failed to exec cmd")?;

    let stream = channel.into_stream();
    let (mut rx, _) = tokio::io::split(stream);

    let mut all: Vec<u8> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = rx.read(&mut chunk).await?;
        if n == 0 { break; }
        all.extend_from_slice(&chunk[..n]);
        pending.extend_from_slice(&chunk[..n]);
        // Emit only whole lines: a chunk boundary can split a line (or even a
        // UTF-8 char), so the tail waits for more bytes.
        while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = pending.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if !line.is_empty() { on_line(line); }
        }
    }
    if !pending.is_empty() {
        let line = String::from_utf8_lossy(&pending);
        let line = line.trim_end_matches('\r');
        if !line.is_empty() { on_line(line); }
    }

    Ok(String::from_utf8_lossy(&all).into_owned())
}
