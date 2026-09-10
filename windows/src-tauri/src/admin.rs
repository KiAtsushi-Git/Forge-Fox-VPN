use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use russh::client;
use russh::ChannelMsg;
use russh_keys::key::PublicKey;
use std::sync::Arc;
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
    let mut channel = session.channel_open_session().await.context("channel_open_session failed")?;
    channel.exec(true, cmd.as_bytes()).await.context("Failed to exec cmd")?;

    // Read stdout AND stderr plus the exit status. The old version stopped at
    // EOF on stdout only: a command that failed (exit != 0) still returned Ok,
    // so "user created" could be reported for a useradd that never ran — the
    // user then vanished from the list and its SSH link was rejected.
    let mut output = String::new();
    let mut exit_code: Option<u32> = None;
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { ref data } => output.push_str(&String::from_utf8_lossy(data)),
            ChannelMsg::ExtendedData { ref data, .. } => {
                output.push_str(&String::from_utf8_lossy(data));
            }
            ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
            _ => {}
        }
    }
    let _ = session
        .disconnect(russh::Disconnect::ByApplication, "done", "en")
        .await;

    check_exit(exit_code, &output)?;
    Ok(output)
}

fn check_exit(exit_code: Option<u32>, output: &str) -> Result<()> {
    match exit_code {
        Some(0) | None => Ok(()),
        Some(code) => {
            // Include the tail of the output — that's where bash prints the error.
            let tail: Vec<&str> = output.lines().collect();
            let tail: String = tail[tail.len().saturating_sub(5)..].join("\n");
            bail!("команда завершилась с кодом {code}: {tail}");
        }
    }
}

/// Same as `run_remote_cmd`, but every complete output line is passed to
/// `on_line` the moment it arrives. Long installs (docker pull, a 5-15 min
/// source build) would otherwise be a black box until the very end — the
/// command output only used to surface when the exec channel closed.
///
/// Returns the full output, like `run_remote_cmd`. Checks the remote exit
/// code — `set -e` scripts that die midway used to still report success.
pub async fn run_remote_cmd_streaming<F>(server: &SshServer, cmd: &str, mut on_line: F) -> Result<String>
where
    F: FnMut(&str),
{
    let mut session = connect(server).await?;
    let mut channel = session.channel_open_session().await.context("channel_open_session failed")?;
    channel.exec(true, cmd.as_bytes()).await.context("Failed to exec cmd")?;

    let mut all: Vec<u8> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut exit_code: Option<u32> = None;
    loop {
        let msg = tokio::select! {
            m = channel.wait() => match m {
                Some(m) => m,
                None => break,
            },
        };
        let chunk: Vec<u8> = match msg {
            ChannelMsg::Data { ref data } => data.to_vec(),
            ChannelMsg::ExtendedData { ref data, .. } => data.to_vec(),
            ChannelMsg::ExitStatus { exit_status } => {
                exit_code = Some(exit_status);
                continue;
            }
            _ => continue,
        };
        all.extend_from_slice(&chunk);
        pending.extend_from_slice(&chunk);
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

    let _ = session
        .disconnect(russh::Disconnect::ByApplication, "done", "en")
        .await;

    let output = String::from_utf8_lossy(&all).into_owned();
    check_exit(exit_code, &output)?;
    Ok(output)
}
