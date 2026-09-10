/// Tauri commands (IPC bridge between Rust backend and frontend).
use crate::config::{self, *};
use crate::vpn;
use tauri::State;
use std::sync::Arc;
use parking_lot::Mutex;

// ── VPN state exposed to frontend ─────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub vpn_status: Arc<Mutex<VpnStatus>>,
    pub start_time: Arc<Mutex<Option<std::time::Instant>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            vpn_status: Arc::new(Mutex::new(VpnStatus::Disconnected)),
            start_time: Arc::new(Mutex::new(None)),
        }
    }
}

// ── Settings commands ─────────────────────────────────────────────────────────
#[tauri::command]
pub fn update_subscription(sub: Subscription) -> Result<(), String> {
    config::update_servers(|s| {
        if let Some(pos) = s.subscriptions.iter().position(|x| x.id == sub.id) {
            s.subscriptions[pos] = sub;
        }
    });
    Ok(())
}

#[tauri::command]
pub fn get_settings() -> AppSettings {
    config::get_settings()
}

#[tauri::command]
pub fn update_settings(settings: AppSettings) -> Result<(), String> {
    for rule in &settings.rules {
        rule.validate()?;
    }
    config::update_settings(|s| *s = settings);
    // Settings carry the ruleset and both split toggles, so a live tunnel has to
    // be reconciled here too, not just from the per-rule commands.
    vpn::refresh_split();
    Ok(())
}

#[tauri::command]
pub fn update_theme(theme: String) -> Result<(), String> {
    config::update_settings(|s| s.theme = theme);
    Ok(())
}

// ── Servers commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_servers() -> ServersStore {
    config::get_servers()
}

#[tauri::command]
pub fn add_server(server: SshServer) -> Result<(), String> {
    config::update_servers(|s| s.servers.push(server));
    Ok(())
}

#[tauri::command]
pub fn update_server(server: SshServer) -> Result<(), String> {
    config::update_servers(|s| {
        if let Some(pos) = s.servers.iter().position(|srv| srv.id == server.id) {
            s.servers[pos] = server;
        }
    });
    Ok(())
}

#[tauri::command]
pub fn delete_server(server_id: String) -> Result<(), String> {
    config::update_servers(|s| s.servers.retain(|srv| srv.id != server_id));
    Ok(())
}

#[tauri::command]
pub fn add_subscription(sub: Subscription) -> Result<(), String> {
    config::update_servers(|s| s.subscriptions.push(sub));
    Ok(())
}

#[tauri::command]
pub fn delete_subscription(sub_id: String) -> Result<(), String> {
    config::update_servers(|s| s.subscriptions.retain(|sub| sub.id != sub_id));
    Ok(())
}

#[tauri::command]
pub fn parse_ssh_link(link: String) -> Option<SshServer> {
    SshServer::from_link(&link)
}

// ── Split-tunnel rules ────────────────────────────────────────────────────────
//
// Every mutation here ends with `vpn::refresh_split()`, which reconciles the
// live tunnel's routes. Before that, rules were only read once at connect time,
// so editing a rule or flipping a toggle did nothing until reconnect.

#[tauri::command]
pub fn add_rule(rule: SplitRule) -> Result<(), String> {
    rule.validate()?;

    let normalized = rule.normalized_value();
    let dup = config::get_settings().rules.iter().any(|r| {
        r.rule_type == rule.rule_type && r.normalized_value() == normalized
    });
    if dup {
        return Err("Такое правило уже есть в списке".into());
    }

    config::update_settings(|s| s.rules.push(rule));
    vpn::refresh_split();
    Ok(())
}

#[tauri::command]
pub fn update_rule(rule: SplitRule) -> Result<(), String> {
    rule.validate()?;

    let normalized = rule.normalized_value();
    let settings = config::get_settings();
    if !settings.rules.iter().any(|r| r.id == rule.id) {
        return Err("Правило не найдено".into());
    }
    let dup = settings.rules.iter().any(|r| {
        r.id != rule.id && r.rule_type == rule.rule_type && r.normalized_value() == normalized
    });
    if dup {
        return Err("Такое правило уже есть в списке".into());
    }

    config::update_settings(|s| {
        if let Some(slot) = s.rules.iter_mut().find(|r| r.id == rule.id) {
            *slot = rule;
        }
    });
    vpn::refresh_split();
    Ok(())
}

#[tauri::command]
pub fn delete_rule(rule_id: String) -> Result<(), String> {
    config::update_settings(|s| s.rules.retain(|r| r.id != rule_id));
    vpn::refresh_split();
    Ok(())
}

#[tauri::command]
pub fn toggle_rule(rule_id: String, enabled: bool) -> Result<(), String> {
    config::update_settings(|s| {
        if let Some(rule) = s.rules.iter_mut().find(|r| r.id == rule_id) {
            rule.enabled = enabled;
        }
    });
    vpn::refresh_split();
    Ok(())
}

#[tauri::command]
pub fn get_rules() -> Vec<SplitRule> {
    config::get_settings().rules
}

#[tauri::command]
pub fn clear_rules() -> Result<(), String> {
    config::update_settings(|s| s.rules.clear());
    vpn::refresh_split();
    Ok(())
}

#[tauri::command]
pub fn set_split_enabled(enabled: bool) -> Result<(), String> {
    config::update_settings(|s| s.split_enabled = enabled);
    vpn::refresh_split();
    Ok(())
}

#[tauri::command]
pub fn set_split_mode(mode: SplitMode) -> Result<(), String> {
    config::update_settings(|s| s.split_mode = mode);
    vpn::refresh_split();
    Ok(())
}

/// What the UI shows on the split-tunnel page: current mode, whether the engine
/// is actually steering traffic right now, and how many routes it holds.
#[derive(serde::Serialize)]
pub struct SplitStatus {
    pub enabled: bool,
    pub mode: SplitMode,
    pub active: bool,
    pub route_count: usize,
    pub total_rules: usize,
    pub enabled_rules: usize,
}

#[tauri::command]
pub fn get_split_status() -> SplitStatus {
    let s = config::get_settings();
    SplitStatus {
        enabled: s.split_enabled,
        mode: s.split_mode,
        active: vpn::is_running() && s.split_enabled,
        route_count: vpn::split_route_count(),
        total_rules: s.rules.len(),
        enabled_rules: s.rules.iter().filter(|r| r.enabled).count(),
    }
}

/// Executables of currently running processes, for the "add app" picker.
#[tauri::command]
pub fn list_running_apps() -> Vec<vpn::process::RunningApp> {
    vpn::process::list_running_apps()
}

/// Plain-text export: one `type:value` per line, `#` comments allowed.
/// Deliberately not JSON so a list can be pasted between machines by hand.
#[tauri::command]
pub fn export_rules() -> String {
    let mut out = String::from("# ForgeFoxVPN split-tunnel rules\n# формат: тип:значение\n");
    for r in config::get_settings().rules {
        if !r.enabled {
            out.push_str("# (выключено) ");
        }
        out.push_str(&format!("{}:{}\n", r.rule_type.as_str(), r.value));
    }
    out
}

#[tauri::command]
pub fn import_rules(text: String, replace: bool) -> Result<usize, String> {
    let mut parsed: Vec<SplitRule> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, value) = line
            .split_once(':')
            .ok_or_else(|| format!("Не разобрать строку: {line}"))?;
        let rule_type = RuleType::from_str(kind.trim())
            .ok_or_else(|| format!("Неизвестный тип правила: {kind}"))?;

        let rule = SplitRule::new(rule_type, value.trim().to_string());
        rule.validate()?;
        parsed.push(rule);
    }

    if parsed.is_empty() {
        return Err("В списке нет ни одного правила".into());
    }

    let added = std::cell::Cell::new(0usize);
    config::update_settings(|s| {
        if replace {
            s.rules.clear();
        }
        let mut count = 0;
        for rule in &parsed {
            let normalized = rule.normalized_value();
            let dup = s.rules.iter().any(|r| {
                r.rule_type == rule.rule_type && r.normalized_value() == normalized
            });
            if !dup {
                s.rules.push(rule.clone());
                count += 1;
            }
        }
        added.set(count);
    });

    vpn::refresh_split();
    Ok(added.get())
}

// ── VPN control ───────────────────────────────────────────────────────────────

#[tauri::command]
pub fn vpn_connect(server_id: String, state: State<AppState>) -> Result<(), String> {
    if vpn::is_running() {
        return Err("VPN already running".into());
    }

    let store = config::get_servers();
    let server = store.servers.iter()
        .chain(store.subscriptions.iter().flat_map(|sub| &sub.servers))
        .find(|s| s.id == server_id)
        .ok_or("Server not found")?
        .clone();

    let settings = config::get_settings();

    // Generate random /24 subnet to avoid conflicts
    let x = rand::random::<u8>() % 241 + 10;
    let y = rand::random::<u8>() % 241 + 10;
    let client_ip = format!("10.{}.{}.2", x, y);
    let server_ip = format!("10.{}.{}.1", x, y);

    let cfg = vpn::ssh_vpn::SshVpnConfig {
        host: server.host,
        port: server.port,
        username: server.username,
        password: server.password,
        client_ip,
        server_tun_ip: server_ip,
        mtu: settings.mtu,
    };

    *state.vpn_status.lock() = VpnStatus::Connecting;
    *state.start_time.lock() = Some(std::time::Instant::now());

    vpn::start(cfg);

    // Give it a moment to establish, then update status
    std::thread::sleep(std::time::Duration::from_millis(500));
    if vpn::is_running() {
        *state.vpn_status.lock() = VpnStatus::Connected;
    }

    Ok(())
}

#[tauri::command]
pub fn vpn_disconnect(state: State<AppState>) -> Result<(), String> {
    vpn::stop();
    *state.vpn_status.lock() = VpnStatus::Disconnected;
    *state.start_time.lock() = None;
    Ok(())
}

#[tauri::command]
pub fn vpn_get_state(state: State<AppState>) -> VpnState {
    let status = state.vpn_status.lock().clone();
    let uptime = state.start_time.lock()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    let (tx, rx) = vpn::get_traffic();

    VpnState {
        status,
        server_name: String::new(),
        server_ip: String::new(),
        uptime_secs: uptime,
        tx_bytes: tx,
        rx_bytes: rx,
        log_tail: Vec::new(),
    }
}

// ── Logs ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_logs() -> Vec<String> {
    crate::vpn::ssh_vpn::get_logs()
}

#[tauri::command]
pub fn clear_logs() {
    crate::vpn::ssh_vpn::clear_logs();
}

// ── Ping server ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn ping_server(server: SshServer) -> Result<u64, String> {
    use std::time::Instant;
    use tokio::net::TcpStream;
    use tokio::time::{timeout, Duration};

    let addr = format!("{}:{}", server.host, server.port);
    let start = Instant::now();

    match timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => Ok(start.elapsed().as_millis() as u64),
        Ok(Err(e)) => Err(format!("Connection failed: {}", e)),
        Err(_) => Err("Timeout".into()),
    }
}

// ── Fetch subscription ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn fetch_subscription(url: String) -> Result<Vec<SshServer>, String> {
    let resp = reqwest::get(&url).await
        .map_err(|e| format!("HTTP error: {}", e))?
        .text().await
        .map_err(|e| format!("Read error: {}", e))?;

    let decoded = if resp.contains("ssh://") {
        resp
    } else {
        // Try base64 decode
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(resp.trim())
            .map_err(|e| format!("Base64 decode error: {}", e))?;
        String::from_utf8(bytes)
            .map_err(|e| format!("UTF-8 error: {}", e))?
    };

    let servers: Vec<SshServer> = decoded
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.contains("ssh://") {
                let link = trimmed[trimmed.find("ssh://").unwrap()..].to_string();
                SshServer::from_link(&link)
            } else {
                None
            }
        })
        .collect();

    if servers.is_empty() {
        Err("No SSH servers found in subscription".into())
    } else {
        Ok(servers)
    }
}

// ── Self-Host Admin ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn sh_install_host(server: SshServer) -> Result<String, String> {
    crate::vpn::ssh_vpn::vpn_log_ext(format!("Начало установки VPN Host на сервер {}...", server.host));
    let base_install = include_str!("../../install.sh");
    
    // We upload the script to /tmp/install.sh so ${BASH_SOURCE[0]} works
    let setup_script = format!(r#"
set -e

cat << 'EOF_INSTALL' > /tmp/forgefox_install.sh
{}
EOF_INSTALL

chmod +x /tmp/forgefox_install.sh
bash /tmp/forgefox_install.sh

# Setup restricted user environment
groupadd -f forgefox

cat << 'EOF' > /usr/local/bin/ff-shell
#!/bin/bash
if [ "$1" = "-c" ]; then
    if [[ "$2" == *"forgefox-bridge"* ]] || [[ "$2" == *"python"* ]]; then
        exec sudo /bin/bash -c "$2"
    fi
fi
echo "Access restricted to ForgeFox VPN."
exit 1
EOF
chmod +x /usr/local/bin/ff-shell

# Give forgefox group passwordless sudo so they can configure TUN and IP
echo "%forgefox ALL=(ALL) NOPASSWD: ALL" > /etc/sudoers.d/forgefox
chmod 0440 /etc/sudoers.d/forgefox

if ! grep -q "Match Group forgefox" /etc/ssh/sshd_config; then
    echo "" >> /etc/ssh/sshd_config
    echo "Match Group forgefox" >> /etc/ssh/sshd_config
    echo "    AllowTcpForwarding no" >> /etc/ssh/sshd_config
    echo "    X11Forwarding no" >> /etc/ssh/sshd_config
    echo "    PermitTunnel yes" >> /etc/ssh/sshd_config
    systemctl restart sshd || systemctl restart ssh
fi

echo "HOST_SETUP_OK"
"#, base_install);

    let res = crate::admin::run_remote_cmd_streaming(&server, &setup_script, |line| {
        crate::vpn::ssh_vpn::vpn_log_ext(format!("[host] {line}"));
    }).await
        .map_err(|e| {
            crate::vpn::ssh_vpn::vpn_log_ext(format!("Ошибка установки Host: {}", e));
            format!("SSH error: {}", e)
        })?;
    crate::vpn::ssh_vpn::vpn_log_ext("Установка VPN Host успешно завершена.");
    Ok(res)
}

/// Single-quote a value for safe interpolation into a remote bash command.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[tauri::command]
pub async fn sh_install_provider(server: SshServer, db_type: String, admin_user: String, admin_pass: String) -> Result<String, String> {
    crate::vpn::ssh_vpn::vpn_log_ext(format!("Начало установки Provider Panel на {}...", server.host));

    if db_type != "sqlite" && db_type != "postgres" {
        return Err(format!("Неизвестный тип БД: {db_type}"));
    }

    // The installer ships with the app instead of being curl'd from GitHub at
    // install time: a blocked/unreachable raw.githubusercontent.com used to make
    // `curl -s | bash` a silent no-op that still reported success.
    let provider_install = include_str!("../../provider_install.sh");

    let setup_script = format!(
        r#"
set -e

cat << 'EOF_INSTALL' > /tmp/forgefox_provider_install.sh
{}
EOF_INSTALL

chmod +x /tmp/forgefox_provider_install.sh
bash /tmp/forgefox_provider_install.sh --db {} --user {} --pass {}
rm -f /tmp/forgefox_provider_install.sh

echo "PROVIDER_INSTALL_DONE"
"#,
        provider_install,
        db_type,
        shell_quote(&admin_user),
        shell_quote(&admin_pass)
    );

    let res = crate::admin::run_remote_cmd_streaming(&server, &setup_script, |line| {
        crate::vpn::ssh_vpn::vpn_log_ext(format!("[provider] {line}"));
    }).await
        .map_err(|e| {
            crate::vpn::ssh_vpn::vpn_log_ext(format!("Ошибка установки Provider: {}", e));
            format!("SSH error: {}", e)
        })?;

    // run_remote_cmd doesn't surface the remote exit code, so the wrapper
    // script only reaches this marker when `set -e` let it finish cleanly.
    if !res.contains("PROVIDER_INSTALL_DONE") {
        crate::vpn::ssh_vpn::vpn_log_ext("Установка Provider прервана (нет маркера успеха). Подробности выше.".to_string());
        return Err("Установка Provider не завершилась успешно — подробности в логах".into());
    }

    crate::vpn::ssh_vpn::vpn_log_ext(format!("Установка Provider Panel успешно завершена. Панель доступна на http://{}:8080", server.host));
    Ok(res)
}


#[tauri::command]
pub async fn sh_clean_host(server: SshServer) -> Result<String, String> {
    crate::vpn::ssh_vpn::vpn_log_ext(format!("Начало полной очистки сервера {}...", server.host));
    let script = r#"
set -x

# 1. Delete users
for user in $(awk -F: '$7 == "/usr/local/bin/ff-shell" {print $1}' /etc/passwd); do
    pkill -u "$user" || true
    userdel -f -r "$user" || true
done

# 2. Delete group
groupdel forgefox || true

# 3. Delete files
rm -f /usr/local/bin/forgefox-bridge
rm -f /usr/local/bin/ff-shell
rm -f /etc/sysctl.d/99-forgefox.conf
rm -rf /etc/forgefox
rm -f /etc/sudoers.d/forgefox
rm -f /tmp/forgefox_install.sh

# 4. Revert sshd
sed -i '/Match Group forgefox/,$d' /etc/ssh/sshd_config || true
systemctl restart sshd || systemctl restart ssh || true

# 5. Revert iptables
PRIMARY_IF=$(ip route show default | awk '{for(i=1;i<NF;i++) if($i=="dev") print $(i+1); exit}')
if [ -n "$PRIMARY_IF" ]; then
    iptables -t nat -D POSTROUTING -s 10.0.0.0/8 -o "$PRIMARY_IF" -j MASQUERADE 2>/dev/null || true
fi
iptables -D FORWARD -s 10.0.0.0/8 -j ACCEPT 2>/dev/null || true
iptables -D FORWARD -d 10.0.0.0/8 -j ACCEPT 2>/dev/null || true
iptables -t mangle -D FORWARD -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null || true

# 6. Remove the Provider panel (containers, volumes, images, files)
if command -v docker >/dev/null 2>&1; then
    if [ -f /opt/forgefox-provider/docker-compose.yml ]; then
        cd /opt/forgefox-provider || true
        docker compose down -v --remove-orphans 2>/dev/null \
            || docker-compose down -v --remove-orphans 2>/dev/null \
            || true
        cd / || true
    fi
    # compose down may not run if the compose file was already deleted
    docker rm -f forgefox_panel forgefox_db 2>/dev/null || true
    docker volume rm forgefox-provider_forgefox_db_data 2>/dev/null || true
    docker rmi ghcr.io/kiatsushi-git/forgefoxvpn-provider:latest forgefox-provider:local 2>/dev/null || true
fi
rm -rf /opt/forgefox-provider
rm -f /tmp/forgefox_provider_install.sh

# Close the panel port again if we (or the installer) opened it
if command -v ufw >/dev/null 2>&1; then
    ufw delete allow 8080/tcp 2>/dev/null || true
fi

echo "CLEAN_OK"
"#;

    let res = crate::admin::run_remote_cmd_streaming(&server, script, |line| {
        crate::vpn::ssh_vpn::vpn_log_ext(format!("[clean] {line}"));
    }).await
        .map_err(|e| {
            crate::vpn::ssh_vpn::vpn_log_ext(format!("Ошибка очистки Host: {}", e));
            format!("SSH error: {}", e)
        })?;
    if !res.contains("CLEAN_OK") {
        crate::vpn::ssh_vpn::vpn_log_ext("Очистка прервана (нет маркера CLEAN_OK). Подробности выше.".to_string());
        return Err("Очистка не завершилась успешно — подробности в логах".into());
    }
    crate::vpn::ssh_vpn::vpn_log_ext("Очистка сервера успешно завершена (Host и Provider).");
    Ok(res)
}

#[derive(serde::Serialize)]
pub struct ShUser {
    pub username: String,
    pub expiry: String,
    pub limit_gb: String,
}

#[tauri::command]
pub async fn sh_list_users(server: SshServer) -> Result<Vec<ShUser>, String> {
    let script = r#"
    for user in $(awk -F: '$7 == "/usr/local/bin/ff-shell" {print $1}' /etc/passwd); do
        exp=$(chage -l "$user" 2>/dev/null | grep 'Account expires' | cut -d: -f2 | xargs)
        if [ "$exp" = "never" ] || [ -z "$exp" ]; then exp="--"; fi
        limit="--"
        if [ -f "/etc/forgefox/limits/$user.gb" ]; then limit=$(cat "/etc/forgefox/limits/$user.gb"); fi
        echo "$user|$exp|$limit"
    done
    "#;
    
    let output = crate::admin::run_remote_cmd(&server, script).await
        .map_err(|e| format!("SSH error: {}", e))?;
    
    let mut users = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.trim().split('|').collect();
        if parts.len() == 3 {
            users.push(ShUser {
                username: parts[0].to_string(),
                expiry: parts[1].to_string(),
                limit_gb: parts[2].to_string(),
            });
        }
    }
    Ok(users)
}

#[tauri::command]
pub async fn sh_add_user(server: SshServer, username: String, expiry: Option<String>, limit_gb: Option<u32>) -> Result<String, String> {
    let password: String = {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();
        (0..12).map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        }).collect()
    };

    let mut bash_cmds = vec![
        format!("useradd -m -g forgefox -s /usr/local/bin/ff-shell {0}", username),
        format!("echo '{0}:{1}' | chpasswd", username, password),
    ];

    if let Some(exp) = expiry {
        if !exp.is_empty() {
            bash_cmds.push(format!("usermod -e {} {}", exp, username));
        }
    }

    if let Some(limit) = limit_gb {
        bash_cmds.push(format!("mkdir -p /etc/forgefox/limits"));
        bash_cmds.push(format!("echo {} > /etc/forgefox/limits/{}.gb", limit, username));
    }

    // Verify the user actually exists afterwards: useradd can fail (e.g. the
    // `forgefox` group is missing because Host install died midway) and the
    // old code still reported success and copied a dead connection link.
    bash_cmds.push(format!("id -u {0} >/dev/null 2>&1 || {{ echo 'USER_NOT_CREATED' >&2; exit 42; }}", username));

    let cmd = bash_cmds.join(" && ");

    crate::admin::run_remote_cmd(&server, &cmd).await
        .map_err(|e| format!("SSH error: {}", e))?;
    
    Ok(password)
}

#[tauri::command]
pub async fn sh_del_user(server: SshServer, username: String) -> Result<String, String> {
    let cmd = format!("userdel -r {}", username);
    let output = crate::admin::run_remote_cmd(&server, &cmd).await
        .map_err(|e| format!("SSH error: {}", e))?;
    Ok(output)
}

#[tauri::command]
pub async fn sh_reset_password(server: SshServer, username: String) -> Result<String, String> {
    let password: String = {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();
        (0..12).map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char).collect()
    };
    let cmd = format!("echo '{}:{}' | chpasswd", username, password);
    crate::admin::run_remote_cmd(&server, &cmd).await
        .map_err(|e| format!("SSH error: {}", e))?;
    Ok(password)
}

#[derive(serde::Serialize)]
pub struct HostStats {
    pub cpu: String,
    pub ram: String,
}

#[tauri::command]
pub async fn sh_get_stats(server: SshServer) -> Result<HostStats, String> {
    let cmd = "cpu=$(grep 'cpu ' /proc/stat | awk '{usage=($2+$4)*100/($2+$4+$5)} END {print int(usage) \"%\"}') && ram=$(free -m | awk '/Mem:/ {print $3\"MB / \"$2\"MB\"}') && echo \"$cpu|$ram\"";
    let output = crate::admin::run_remote_cmd(&server, cmd).await
        .map_err(|e| format!("SSH error: {}", e))?;
    let parts: Vec<&str> = output.trim().split('|').collect();
    if parts.len() == 2 {
        Ok(HostStats {
            cpu: parts[0].to_string(),
            ram: parts[1].to_string(),
        })
    } else {
        Err("Failed to parse stats".into())
    }
}
