use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── SSH Server ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshServer {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub ping_ms: Option<u64>,
}

impl SshServer {
    pub fn new(name: String, host: String, port: u16, username: String, password: String) -> Self {
        Self { id: Uuid::new_v4().to_string(), name, host, port, username, password, ping_ms: None }
    }

    /// Parse ssh://user:pass@host:port#name
    pub fn from_link(link: &str) -> Option<Self> {
        let link = link.trim();
        let rest = link.strip_prefix("ssh://")?;

        let (main, fragment) = rest.split_once('#').unwrap_or((rest, ""));
        let name = if fragment.is_empty() {
            "SSH Server".to_string()
        } else {
            urlencoding::decode(fragment).unwrap_or_default().into_owned()
        };

        // Считаем количество символов '@', чтобы понять формат ссылки
        let at_count = main.chars().filter(|&c| c == '@').count();

        if at_count == 2 {
            // ТВОЙ КАСТОМНЫЙ ФОРМАТ: user@password:host@port
            let (username, rest_after_user) = main.split_once('@')?;
            let (middle, port_str) = rest_after_user.rsplit_once('@')?;
            let port = port_str.parse::<u16>().unwrap_or(22);
            let (password, host) = middle.rsplit_once(':').unwrap_or(("", middle));

            return Some(Self::new(
                name, host.to_string(), port, username.to_string(), password.to_string()
            ));
        }

        // СТАНДАРТНЫЙ ФОРМАТ: user:password@host:port (и фоллбэк для подписок)
        if let Some(last_at) = main.rfind('@') {
            let auth = &main[..last_at];
            let host_port = &main[last_at + 1..];

            let (username, password) = auth
                .split_once(':')
                .unwrap_or((auth, ""));

            let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
                (h.to_string(), p.parse::<u16>().unwrap_or(22))
            } else {
                (host_port.to_string(), 22)
            };

            return Some(Self::new(
                name, host.to_string(), port, username.to_string(), password.to_string()
            ));
        }

        None
    }

    pub fn to_link(&self) -> String {
        format!(
            "ssh://{}:{}@{}:{}#{}",
            self.username,
            self.password,
            self.host,
            self.port,
            urlencoding::encode(&self.name)
        )
    }
}

// ── Subscription ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
    pub servers: Vec<SshServer>,
}

// ── Split-tunnel rules ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SplitMode {
    /// All traffic via VPN, except the list
    Bypass,
    /// Only listed traffic via VPN
    Proxy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleType {
    Ip,
    Cidr,
    Domain,
    /// Domain zone: the apex plus every subdomain (`*.example.com`)
    DomainZone,
    App,
}

impl RuleType {
    /// Wire name, matching the serde representation and the UI's `<option value>`.
    pub fn as_str(&self) -> &'static str {
        match self {
            RuleType::Ip => "ip",
            RuleType::Cidr => "cidr",
            RuleType::Domain => "domain",
            RuleType::DomainZone => "domain_zone",
            RuleType::App => "app",
        }
    }

    /// Parse a wire name. Accepts a few friendly aliases so a hand-written
    /// import list does not fail on `zone` or `subnet`.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ip" => Some(RuleType::Ip),
            "cidr" | "subnet" | "net" => Some(RuleType::Cidr),
            "domain" | "site" => Some(RuleType::Domain),
            "domain_zone" | "domainzone" | "zone" => Some(RuleType::DomainZone),
            "app" | "exe" | "process" => Some(RuleType::App),
            _ => None,
        }
    }

    /// Russian label for the UI and log lines.
    pub fn label(&self) -> &'static str {
        match self {
            RuleType::Ip => "IP-адрес",
            RuleType::Cidr => "Подсеть",
            RuleType::Domain => "Сайт",
            RuleType::DomainZone => "Доменная зона",
            RuleType::App => "Приложение",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitRule {
    pub id: String,
    pub rule_type: RuleType,
    /// IP, CIDR, domain string, or absolute path to .exe
    pub value: String,
    pub enabled: bool,
    /// Optional user-facing note (e.g. why the rule exists)
    #[serde(default)]
    pub note: String,
}

impl SplitRule {
    pub fn new(rule_type: RuleType, value: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            rule_type,
            value,
            enabled: true,
            note: String::new(),
        }
    }

    /// Normalize user input so matching is predictable.
    pub fn normalized_value(&self) -> String {
        let v = self.value.trim();
        match self.rule_type {
            RuleType::Domain | RuleType::DomainZone => v
                .trim_start_matches("*.")
                .trim_matches('.')
                .to_ascii_lowercase(),
            RuleType::App => v.to_ascii_lowercase().replace('/', "\\"),
            _ => v.to_string(),
        }
    }

    /// Reject values that can't possibly work, so bad rules fail loudly in the UI
    /// rather than silently doing nothing at connect time.
    pub fn validate(&self) -> Result<(), String> {
        let v = self.normalized_value();
        if v.is_empty() {
            return Err("Значение правила пусто".into());
        }
        match self.rule_type {
            RuleType::Ip => v
                .parse::<std::net::Ipv4Addr>()
                .map(|_| ())
                .map_err(|_| format!("«{v}» — не похоже на IPv4-адрес")),
            RuleType::Cidr => {
                let (net, prefix) = v
                    .split_once('/')
                    .ok_or_else(|| format!("«{v}» — нужен формат 10.0.0.0/8"))?;
                net.parse::<std::net::Ipv4Addr>()
                    .map_err(|_| format!("«{net}» — не похоже на IPv4-адрес"))?;
                let p: u8 = prefix
                    .parse()
                    .map_err(|_| format!("«{prefix}» — некорректная длина префикса"))?;
                if p > 32 {
                    return Err("Префикс должен быть в диапазоне 0–32".into());
                }
                Ok(())
            }
            RuleType::Domain => {
                if !v.contains('.') || v.starts_with('.') {
                    return Err(format!("«{v}» — не похоже на домен"));
                }
                if v.chars().any(|c| c.is_whitespace() || c == '/' || c == ':') {
                    return Err(format!("«{v}» содержит недопустимые символы"));
                }
                Ok(())
            }
            RuleType::DomainZone => {
                // TLDs might not contain a dot (e.g. 'ru' or 'com')
                if v.chars().any(|c| c.is_whitespace() || c == '/' || c == ':') {
                    return Err(format!("«{v}» содержит недопустимые символы"));
                }
                Ok(())
            }
            RuleType::App => {
                if !v.ends_with(".exe") {
                    return Err("Укажите путь к .exe или его имя (chrome.exe)".into());
                }
                Ok(())
            }
        }
    }
}

// ── App-wide settings ─────────────────────────────────────────────────────────

/// Every field is `#[serde(default)]` so a settings.json written by an older
/// build still loads instead of being silently replaced by defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub selected_server_id: Option<String>,
    pub split_enabled: bool,
    pub split_mode: SplitMode,
    pub rules: Vec<SplitRule>,
    pub theme: String,
    pub auto_reconnect: bool,
    pub minimize_to_tray: bool,
    pub start_on_boot: bool,
    pub log_max_lines: usize,
    pub dns_servers: Vec<String>,
    pub mtu: u16,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            selected_server_id: None,
            split_enabled: false,
            split_mode: SplitMode::Bypass,
            rules: Vec::new(),
            theme: "dark".to_string(),
            auto_reconnect: true,
            minimize_to_tray: true,
            start_on_boot: false,
            log_max_lines: 500,
            dns_servers: vec!["8.8.8.8".to_string(), "1.1.1.1".to_string()],
            mtu: 1400,
        }
    }
}

// ── App-wide servers store ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServersStore {
    pub servers: Vec<SshServer>,
    pub subscriptions: Vec<Subscription>,
}

// ── VPN runtime state (sent to frontend via events) ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VpnStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnState {
    pub status: VpnStatus,
    pub server_name: String,
    pub server_ip: String,
    pub uptime_secs: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub log_tail: Vec<String>,
}

impl Default for VpnState {
    fn default() -> Self {
        Self {
            status: VpnStatus::Disconnected,
            server_name: String::new(),
            server_ip: String::new(),
            uptime_secs: 0,
            tx_bytes: 0,
            rx_bytes: 0,
            log_tail: Vec::new(),
        }
    }
}
