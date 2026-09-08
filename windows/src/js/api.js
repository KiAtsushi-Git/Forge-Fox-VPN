/**
 * api.js — thin wrappers around Tauri invoke() calls.
 * All functions return Promises.
 */

const getInvoke = () => {
  if (window.__TAURI__ && window.__TAURI__.core) return window.__TAURI__.core.invoke;
  if (window.__TAURI__) return window.__TAURI__.invoke;
  return () => Promise.reject("Tauri API not found");
};

// ── Settings ──────────────────────────────────────────────────────────────────
window.Api = {
  invoke: (cmd, args) => getInvoke()(cmd, args),

  // Settings
  getSettings:    ()       => getInvoke()('get_settings'),
  updateSettings: (s)      => getInvoke()('update_settings', { settings: s }),
  updateTheme:    (theme)  => getInvoke()('update_theme', { theme }),

  // Servers
  getServers:         ()      => getInvoke()('get_servers'),
  addServer:          (srv)   => getInvoke()('add_server',    { server: srv }),
  updateServer:       (srv)   => getInvoke()('update_server', { server: srv }),
  deleteServer:       (id)    => getInvoke()('delete_server', { serverId: id }),
  addSubscription:    (sub)   => getInvoke()('add_subscription',    { sub }),
  updateSubscription: (sub)   => getInvoke()('update_subscription', { sub }),
  deleteSubscription: (id)    => getInvoke()('delete_subscription', { subId: id }),
  parseSshLink:       (link)  => getInvoke()('parse_ssh_link', { link }),
  fetchSubscription:  (url)   => getInvoke()('fetch_subscription',  { url }),

  // Rules
  addRule:    (rule)  => getInvoke()('add_rule',    { rule }),
  updateRule: (rule)  => getInvoke()('update_rule', { rule }),
  deleteRule: (id)    => getInvoke()('delete_rule', { ruleId: id }),
  toggleRule: (id, e) => getInvoke()('toggle_rule', { ruleId: id, enabled: e }),
  getRules:   ()      => getInvoke()('get_rules'),
  clearRules: ()      => getInvoke()('clear_rules'),

  // Split tunnel
  setSplitEnabled: (enabled) => getInvoke()('set_split_enabled', { enabled }),
  setSplitMode:    (mode)    => getInvoke()('set_split_mode',    { mode }),
  getSplitStatus:  ()        => getInvoke()('get_split_status'),
  listRunningApps: ()        => getInvoke()('list_running_apps'),
  exportRules:     ()        => getInvoke()('export_rules'),
  importRules:     (text, replace) => getInvoke()('import_rules', { text, replace }),

  // VPN
  vpnConnect:    (serverId) => getInvoke()('vpn_connect',    { serverId }),
  vpnDisconnect: ()         => getInvoke()('vpn_disconnect'),
  vpnGetState:   ()         => getInvoke()('vpn_get_state'),

  // Logs
  getLogs:   () => getInvoke()('get_logs'),
  clearLogs: () => getInvoke()('clear_logs'),

  // Utils
  pingServer: (srv) => getInvoke()('ping_server', { server: srv }),
};

// Tauri window controls
window.TauriWindow = {
  async minimize() {
    const { getCurrentWindow } = window.__TAURI__.window;
    await getCurrentWindow().minimize();
  },
  async toggleMaximize() {
    const { getCurrentWindow } = window.__TAURI__.window;
    const win = getCurrentWindow();
    const isMax = await win.isMaximized();
    if (isMax) await win.unmaximize(); else await win.maximize();
  },
  async close() {
    const { getCurrentWindow } = window.__TAURI__.window;
    await getCurrentWindow().close();
  },
};
