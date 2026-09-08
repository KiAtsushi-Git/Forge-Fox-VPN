/**
 * app.js — ForgeFox VPN desktop application main logic.
 * Navigation, state management, UI rendering.
 */

// ── State ─────────────────────────────────────────────────────────────────────
let state = {
  settings: null,
  servers: null,
  vpnState: null,
  selectedServerId: null,
  pollTimer: null,
  splitStatus: null,
  rulesFilter: 'all',
  rulesSearch: '',
  runningApps: [],
  editingRuleId: null,
};

// ── Utility ───────────────────────────────────────────────────────────────────
function $(sel, ctx = document) { return ctx.querySelector(sel); }
function $$(sel, ctx = document) { return [...ctx.querySelectorAll(sel)]; }

function showToast(msg, type = 'info', ms = 2800) {
  const t = $('#toast');
  t.textContent = msg;
  t.className = `toast ${type}`;
  clearTimeout(t._timer);
  t._timer = setTimeout(() => t.classList.add('hidden'), ms);
}

function closeModal(id) { $(`#${id}`).classList.add('hidden'); }
function openModal(id)  { $(`#${id}`).classList.remove('hidden'); }

function formatBytes(b) {
  if (b < 1024) return `${b} B`;
  if (b < 1024*1024) return `${(b/1024).toFixed(1)} KB`;
  if (b < 1024*1024*1024) return `${(b/1048576).toFixed(1)} MB`;
  return `${(b/1073741824).toFixed(2)} GB`;
}
function formatTime(s) {
  const h = Math.floor(s/3600), m = Math.floor((s%3600)/60), sec = s%60;
  return h > 0 ? `${h}:${String(m).padStart(2,'0')}:${String(sec).padStart(2,'0')}`
               : `${m}:${String(sec).padStart(2,'0')}`;
}
function pingColor(ms) {
  if (ms === null || ms === undefined) return 'ping-gray';
  if (ms < 100) return 'ping-good';
  if (ms < 300) return 'ping-ok';
  return 'ping-bad';
}

// ── Routing ───────────────────────────────────────────────────────────────────
function goToPage(name) {
  $$('.page').forEach(p => p.classList.remove('active'));
  $$('.nav-item').forEach(n => n.classList.remove('active'));
  $(`#page-${name}`)?.classList.add('active');
  $(`[data-page="${name}"]`)?.classList.add('active');

  // Update nav icons
  const icons = { dashboard:'⬡', servers:'⬡', rules:'⬡', logs:'⬡', settings:'⬡', selfhost:'⬡' };
  const filled = { dashboard:'⬢', servers:'⬢', rules:'⬢', logs:'⬢', settings:'⬢', selfhost:'⬢' };
  $$('.nav-item').forEach(n => {
    const p = n.dataset.page;
    n.querySelector('.nav-icon').textContent = p === name ? filled[p] : icons[p];
  });

  if (name === 'servers') renderServers();
  if (name === 'rules')   renderRules();
  if (name === 'logs')    { renderLogs(); scrollLogsBottom(); }
  if (name === 'dashboard') refreshSplitStatus();
}
window.goToPage = goToPage; // expose for inline onclick

// ── Titlebar ──────────────────────────────────────────────────────────────────
function initTitlebar() {
  $('#btn-minimize')?.addEventListener('click', () => TauriWindow.minimize());
  $('#btn-maximize')?.addEventListener('click', () => TauriWindow.toggleMaximize());
  $('#btn-close')?.addEventListener('click',    () => TauriWindow.close());
}

// ── Theme ─────────────────────────────────────────────────────────────────────
function applyTheme(name) {
  $('#theme-css').href = `css/themes/${name}.css`;
}

// ── Dashboard ─────────────────────────────────────────────────────────────────
function initDashboard() {
  $('#btn-connect').addEventListener('click', handleConnectToggle);
  $('#toggle-split').addEventListener('change', async (e) => {
    const on = e.target.checked;
    try {
      // Dedicated command rather than a whole-settings write: it reconciles the
      // live tunnel's routes without racing an unrelated settings edit.
      await Api.setSplitEnabled(on);
      state.settings.split_enabled = on;
      await refreshSplitStatus();
      showToast(on ? 'Правила исключений включены' : 'Правила исключений выключены', 'success');
    } catch (err) {
      e.target.checked = !on;
      showToast(`Ошибка: ${err}`, 'error');
    }
  });
}

async function handleConnectToggle() {
  if (!state.vpnState) return;
  const isConnected = ['connected', 'connecting'].includes(state.vpnState.status);

  if (isConnected) {
    try {
      setConnectingUI('Отключение…');
      await Api.vpnDisconnect();
      setDisconnectedUI();
    } catch(e) { showToast(`Ошибка отключения: ${e}`, 'error'); }
  } else {
    if (!state.selectedServerId) {
      showToast('Выберите сервер сначала', 'error');
      goToPage('servers');
      return;
    }
    try {
      setConnectingUI('Подключение…');
      await Api.vpnConnect(state.selectedServerId);
    } catch(e) {
      setDisconnectedUI();
      showToast(`Ошибка: ${e}`, 'error');
    }
  }
}

// The connect button holds an inline SVG, so state is expressed with a class on
// the wrapper (which spins it) rather than by swapping glyph text.
function setIconBusy(busy) {
  $('#connect-icon')?.classList.toggle('busy', busy);
}

function setConnectingUI(label) {
  $('#status-ring').classList.remove('connected');
  $('#status-label').textContent = label.toUpperCase();
  $('#status-label').classList.remove('connected');
  setIconBusy(true);
  $('#btn-connect').classList.remove('active');
  $('#status-sub').textContent = '…';
}
function setConnectedUI(serverName) {
  $('#status-ring').classList.add('connected');
  $('#status-label').textContent = 'ЗАЩИЩЕНО';
  $('#status-label').classList.add('connected');
  setIconBusy(false);
  $('#btn-connect').classList.add('active');
  $('#status-sub').textContent = serverName || 'VPN активен';
  $('#stats-row').style.display = 'flex';
}
function setDisconnectedUI() {
  $('#status-ring').classList.remove('connected');
  $('#status-label').textContent = 'НЕ ЗАЩИЩЕНО';
  $('#status-label').classList.remove('connected');
  setIconBusy(false);
  $('#btn-connect').classList.remove('active');
  $('#status-sub').textContent = 'Нажмите для подключения';
  $('#stats-row').style.display = 'none';
}

function updateStatsUI(vpnState) {
  if (!vpnState) return;
  const { status, uptime_secs, tx_bytes, rx_bytes } = vpnState;
  if (status === 'connected') {
    $('#stat-uptime').textContent = formatTime(uptime_secs);
    $('#stat-tx').textContent = formatBytes(tx_bytes);
    $('#stat-rx').textContent = formatBytes(rx_bytes);
  }
}

function updateSelectedServerInfo() {
  const store = state.servers;
  if (!store || !state.selectedServerId) {
    $('#sel-server-name').textContent = 'Не выбран';
    $('#sel-server-addr').textContent = '─';
    return;
  }
  const all = [
    ...store.servers,
    ...store.subscriptions.flatMap(s => s.servers),
  ];
  const srv = all.find(s => s.id === state.selectedServerId);
  if (srv) {
    $('#sel-server-name').textContent = srv.name;
    $('#sel-server-addr').textContent = `${srv.host}:${srv.port}`;
  }
}

// ── Poll VPN state + logs ──────────────────────────────────────────────────────
let lastLogCount = 0;

function startPolling() {
  stopPolling();
  state.pollTimer = setInterval(async () => {
    try {
      // VPN state
      const vpnState = await Api.vpnGetState();
      state.vpnState = vpnState;
      const { status } = vpnState;
      if (status === 'connected') {
        const store = state.servers;
        const all = store ? [
          ...store.servers,
          ...store.subscriptions.flatMap(s => s.servers),
        ] : [];
        const srv = all.find(s => s.id === state.selectedServerId);
        setConnectedUI(srv?.name);
        updateStatsUI(vpnState);
      } else if (status === 'reconnecting') {
        setConnectingUI('Переподключение…');
      } else if (status === 'disconnected') {
        setDisconnectedUI();
      }

      // Route count changes as DNS answers and app connections are observed,
      // so the split status is polled alongside the VPN state.
      if ($('#page-rules').classList.contains('active') ||
          $('#page-dashboard').classList.contains('active')) {
        await refreshSplitStatus();
      }

      // Logs (only sync if count changed)
      const allLogs = await Api.getLogs();
      if (allLogs.length !== lastLogCount) {
        lastLogCount = allLogs.length;
        // Replace in-memory log array (keep references)
        logs.length = 0;
        const now = new Date().toLocaleTimeString();
        allLogs.forEach(m => logs.push({ time: now, msg: m }));
        if ($('#page-logs').classList.contains('active')) {
          renderLogs();
          scrollLogsBottom();
        }
      }
    } catch (_) {}
  }, 1000);
}

function stopPolling() {
  if (state.pollTimer) { clearInterval(state.pollTimer); state.pollTimer = null; }
}

// ── Servers page ──────────────────────────────────────────────────────────────

// parse_ssh_link mints a fresh UUID for every server it returns, so a plain
// refresh would renumber the whole subscription — and settings.selected_server_id
// (plus any ping already measured) points at the old numbering. Match each
// incoming server to its previous self by endpoint and carry the old id over.
function reuseServerIds(oldServers, newServers) {
  const byEndpoint = new Map(
    (oldServers || []).map(s => [`${s.host}|${s.port}|${s.username}`, s])
  );
  return (newServers || []).map(srv => {
    const prev = byEndpoint.get(`${srv.host}|${srv.port}|${srv.username}`);
    if (!prev) return srv;
    byEndpoint.delete(`${srv.host}|${srv.port}|${srv.username}`); // one-to-one
    return { ...srv, id: prev.id, ping_ms: srv.ping_ms ?? prev.ping_ms ?? null };
  });
}

function renderServers() {
  const list = $('#server-list');
  if (!list || !state.servers) return;
  const q = ($('#server-search')?.value || '').toLowerCase();

  const allSingleServers = (state.servers.servers || [])
    .filter(s => !q || s.name.toLowerCase().includes(q) || s.host.includes(q));
  const allSubs = (state.servers.subscriptions || []);

  list.innerHTML = '';

  // Single servers
  for (const srv of allSingleServers) {
    list.appendChild(buildServerItem(srv, false));
  }

  // Subscriptions
  for (const sub of allSubs) {
    const header = document.createElement('div');
    header.className = 'sub-header';
    header.innerHTML = `
      <span style="font-weight:600">${escHtml(sub.name)}</span>
      <span class="sub-arrow" id="sub-arrow-${sub.id}">▼ ${sub.servers.length} серверов</span>
      <button class="icon-btn" title="Обновить" data-sub-refresh="${sub.id}">↻</button>
      <button class="icon-btn" title="Удалить" data-sub-del="${sub.id}">🗑</button>
    `;
    const children = document.createElement('div');
    children.className = 'sub-children';
    children.id = `sub-children-${sub.id}`;
    children.style.display = 'none';

    header.addEventListener('click', (e) => {
      if (e.target.dataset.subDel || e.target.dataset.subRefresh) return;
      const isOpen = children.style.display !== 'none';
      children.style.display = isOpen ? 'none' : 'flex';
      $(`#sub-arrow-${sub.id}`).textContent = (isOpen ? '▼' : '▲') + ` ${sub.servers.length} серверов`;
    });

    // Re-fetch the subscription URL and replace its server list in place, keeping
    // the same id so the row (and any selection pointing into it) survives.
    header.querySelector(`[data-sub-refresh="${sub.id}"]`)?.addEventListener('click', async (e) => {
      e.stopPropagation();
      const btn = e.currentTarget;
      if (btn.disabled) return;

      if (!sub.url) {
        showToast('У этой подписки не сохранён URL — обновить нечем', 'error');
        return;
      }

      btn.disabled = true;
      btn.classList.add('spinning');
      try {
        const fetched = await Api.fetchSubscription(sub.url);
        const servers = reuseServerIds(sub.servers, fetched);
        await Api.updateSubscription({ ...sub, servers });
        state.servers = await Api.getServers();
        renderServers();
        showToast(`«${sub.name}»: обновлено, серверов ${servers.length}`, 'success');
      } catch (err) {
        showToast(`Не удалось обновить «${sub.name}»: ${err}`, 'error');
        btn.disabled = false;
        btn.classList.remove('spinning');
      }
    });

    header.querySelector(`[data-sub-del="${sub.id}"]`)?.addEventListener('click', async (e) => {
      e.stopPropagation();
      if (!confirm(`Удалить подписку "${sub.name}"?`)) return;
      try {
        await Api.deleteSubscription(sub.id);
        state.servers = await Api.getServers();
        renderServers();
        showToast('Подписка удалена', 'success');
      } catch (err) { showToast(`Ошибка: ${err}`, 'error'); }
    });

    const filteredChildren = sub.servers.filter(s => !q || s.name.toLowerCase().includes(q) || s.host.includes(q));
    for (const srv of filteredChildren) {
      children.appendChild(buildServerItem(srv, true));
    }

    list.appendChild(header);
    list.appendChild(children);
  }

  if (list.children.length === 0) {
    list.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-muted)">Серверов нет. Нажмите «+ Добавить».</div>';
  }
}

function buildServerItem(srv, inSub) {
  const div = document.createElement('div');
  div.className = 'server-item' + (srv.id === state.selectedServerId ? ' selected' : '');
  div.dataset.id = srv.id;
  const pingText = srv.ping_ms != null ? `${srv.ping_ms}ms` : 'Ping';
  const pingCls  = srv.ping_ms != null ? pingColor(srv.ping_ms) : 'ping-gray';
  div.innerHTML = `
    <div class="server-item-info">
      <div class="server-name">${escHtml(srv.name)}</div>
      <div class="server-addr">${escHtml(srv.host)}:${srv.port}</div>
    </div>
    <span class="server-ping ${pingCls}" id="ping-${srv.id}">${pingText}</span>
    <div class="server-actions">
      <button class="icon-btn" title="Пинг" data-ping="${srv.id}">⚡</button>
      ${!inSub ? `<button class="icon-btn" title="Удалить" data-del="${srv.id}">🗑</button>` : ''}
    </div>
  `;

  div.addEventListener('click', async (e) => {
    if (e.target.dataset.ping || e.target.dataset.del) return;
    state.selectedServerId = srv.id;
    state.settings.selected_server_id = srv.id;
    await Api.updateSettings(state.settings);
    updateSelectedServerInfo();
    renderServers();
    showToast(`Выбран: ${srv.name}`, 'success');
    goToPage('dashboard');
  });

  div.querySelector(`[data-ping="${srv.id}"]`)?.addEventListener('click', async (e) => {
    e.stopPropagation();
    const pingEl = $(`#ping-${srv.id}`);
    pingEl.textContent = '…';
    pingEl.className = 'server-ping ping-gray';
    try {
      const ms = await Api.pingServer(srv);
      srv.ping_ms = ms;
      pingEl.textContent = `${ms}ms`;
      pingEl.className = `server-ping ${pingColor(ms)}`;
    } catch (err) {
      pingEl.textContent = 'Err';
      pingEl.className = 'server-ping ping-bad';
      showToast(`Ping error: ${err}`, 'error');
    }
  });

  div.querySelector(`[data-del="${srv.id}"]`)?.addEventListener('click', async (e) => {
    e.stopPropagation();
    if (!confirm(`Удалить сервер "${srv.name}"?`)) return;
    await Api.deleteServer(srv.id);
    state.servers = await Api.getServers();
    if (state.selectedServerId === srv.id) {
      state.selectedServerId = null;
      updateSelectedServerInfo();
    }
    renderServers();
  });

  return div;
}

function initServersPage() {
  // Add server button
  $('#btn-add-server').addEventListener('click', () => {
    $('#srv-link').value = '';
    $('#srv-name').value = '';
    $('#srv-host').value = '';
    $('#srv-port').value = '22';
    $('#srv-user').value = '';
    $('#srv-pass').value = '';
    $('#modal-server-title').textContent = 'Добавить сервер';
    openModal('modal-server');
  });

  // Parse SSH link
  $('#btn-parse-link').addEventListener('click', async () => {
    const link = $('#srv-link').value.trim();
    if (!link) return;
    try {
      const srv = await Api.parseSshLink(link);
      if (srv) {
        $('#srv-name').value = srv.name;
        $('#srv-host').value = srv.host;
        $('#srv-port').value = srv.port;
        $('#srv-user').value = srv.username;
        $('#srv-pass').value = srv.password;
      } else { showToast('Не удалось распознать ссылку', 'error'); }
    } catch(e) { showToast(`Ошибка: ${e}`, 'error'); }
  });

  // Save server
  $('#btn-save-server').addEventListener('click', async () => {
    const name = $('#srv-name').value.trim() || $('#srv-host').value;
    const host = $('#srv-host').value.trim();
    const port = parseInt($('#srv-port').value) || 22;
    const user = $('#srv-user').value.trim() || 'root';
    const pass = $('#srv-pass').value;
    if (!host) { showToast('Укажите хост', 'error'); return; }
    try {
      const srv = { id: crypto.randomUUID(), name, host, port, username: user, password: pass, ping_ms: null };
      await Api.addServer(srv);
      state.servers = await Api.getServers();
      closeModal('modal-server');
      renderServers();
      showToast('Сервер добавлен', 'success');
    } catch(e) { showToast(`Ошибка: ${e}`, 'error'); }
  });

  // Add subscription
  $('#btn-add-sub').addEventListener('click', () => {
    $('#sub-url').value = '';
    $('#sub-name').value = '';
    openModal('modal-sub');
  });

  $('#btn-fetch-sub').addEventListener('click', async () => {
    const url  = $('#sub-url').value.trim();
    const name = $('#sub-name').value.trim() || url.split('/')[2] || 'Подписка';
    if (!url) { showToast('Укажите URL', 'error'); return; }
    try {
      showToast('Загружаю подписку…');
      const servers = await Api.fetchSubscription(url);
      const sub = { id: crypto.randomUUID(), name, url, servers };
      await Api.addSubscription(sub);
      state.servers = await Api.getServers();
      closeModal('modal-sub');
      renderServers();
      showToast(`Загружено ${servers.length} серверов`, 'success');
    } catch(e) { showToast(`Ошибка: ${e}`, 'error'); }
  });

  // Search
  $('#server-search').addEventListener('input', () => renderServers());

  // Close modals on backdrop click
  $$('.modal').forEach(m => m.addEventListener('click', (e) => {
    if (e.target === m) m.classList.add('hidden');
  }));
}

// ── Self-Host page ─────────────────────────────────────────────────────────────
let currentShServer = null;

function saveShUserPass(serverId, username, pass) {
  const key = `sh_pass_${serverId}_${username}`;
  localStorage.setItem(key, pass);
}

function getShUserPass(serverId, username) {
  const key = `sh_pass_${serverId}_${username}`;
  return localStorage.getItem(key);
}

function removeShUserPass(serverId, username) {
  const key = `sh_pass_${serverId}_${username}`;
  localStorage.removeItem(key);
}

// While an install runs, mirror the newest backend log line onto the button
// so the user sees live progress without switching to the Logs page.
// `logs` is kept in sync by the 1s poller in startPolling.
function startInstallProgressTicker(getTextEl) {
  return setInterval(() => {
    const el = getTextEl();
    if (!el) return;
    const last = logs[logs.length - 1];
    if (last) {
      const t = last.msg.length > 70 ? last.msg.slice(0, 70) + '…' : last.msg;
      el.textContent = t;
    }
  }, 1000);
}

function initSelfHostPage() {
  $('#btn-add-selfhost').addEventListener('click', () => {
    $('#sh-name').value = '';
    $('#sh-host').value = '';
    $('#sh-port').value = '22';
    $('#sh-user').value = '';
    $('#sh-pass').value = '';
    openModal('modal-selfhost');
  });

  $('#btn-save-selfhost').addEventListener('click', () => {
    const name = $('#sh-name').value.trim();
    const host = $('#sh-host').value.trim();
    const port = parseInt($('#sh-port').value.trim()) || 22;
    const user = $('#sh-user').value.trim();
    const pass = $('#sh-pass').value;

    if (!host) { showToast('Укажите хост', 'error'); return; }

    const srv = {
      id: crypto.randomUUID(),
      name: name || host,
      host,
      port,
      username: user || 'root',
      password: pass,
      installed: false
    };

    const servers = JSON.parse(localStorage.getItem('selfhost_servers') || '[]');
    servers.push(srv);
    localStorage.setItem('selfhost_servers', JSON.stringify(servers));
    closeModal('modal-selfhost');
    renderSelfHostServers();
    showToast('Сервер добавлен', 'success');
  });

  $('#btn-install-host').addEventListener('click', async () => {
    if (!currentShServer) return;
    const btn = $('#btn-install-host');
    btn.style.opacity = '0.5';
    btn.style.pointerEvents = 'none';
    
    const originalHtml = btn.innerHTML;
    btn.innerHTML = `
      <div style="display:flex; align-items:center; justify-content:center; gap:12px; padding: 10px 0;">
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="animation: spin 1s linear infinite;">
          <path d="M21 12a9 9 0 1 1-6.219-8.56"></path>
        </svg>
        <div id="host-install-status" style="font-size:15px; font-weight:500;">Установка... следите за ходом в «Логах»</div>
      </div>
      <style>@keyframes spin { 100% { transform: rotate(360deg); } }</style>
    `;

    const progressTimer = startInstallProgressTicker(() => $('#host-install-status'));

    showToast('Начинаем установку Host (ход выполнения — в разделе «Логи»)...', 'info', 10000);
    try {
      await Api.invoke('sh_install_host', { server: currentShServer });
      showToast('Успешно установлено!', 'success');

      const servers = JSON.parse(localStorage.getItem('selfhost_servers') || '[]');
      const s = servers.find(x => x.id === currentShServer.id);
      if (s) { s.installed = true; }
      localStorage.setItem('selfhost_servers', JSON.stringify(servers));
      renderSelfHostServers();
      closeModal('modal-sh-install');
    } catch (e) {
      showToast(`Ошибка: ${e}`, 'error');
    }
    clearInterval(progressTimer);
    btn.innerHTML = originalHtml;
    btn.style.opacity = '1';
    btn.style.pointerEvents = 'auto';
  });

  $('#btn-install-provider').addEventListener('click', () => {
    const form = $('#provider-setup-form');
    if (form.style.display === 'none') {
      form.style.display = 'block';
    } else {
      form.style.display = 'none';
    }
  });

  $('#btn-start-provider-install').addEventListener('click', async () => {
    const dbType = $('#prov-db-type').value;
    const adminUser = $('#prov-admin-user').value.trim();
    const adminPass = $('#prov-admin-pass').value.trim();
    
    if (!adminUser || !adminPass) {
      return showToast('Введите логин и пароль для админа', 'error');
    }
    
    const btn = $('#btn-start-provider-install');
    btn.disabled = true;
    btn.textContent = 'Установка... следите за ходом в «Логах»';

    const progressTimer = startInstallProgressTicker(() => btn);

    showToast('Начинаем установку Provider (ход выполнения — в разделе «Логи»)...', 'info', 10000);

    try {
      await Api.invoke('sh_install_provider', {
        server: currentShServer,
        dbType: dbType,
        adminUser: adminUser,
        adminPass: adminPass
      });
      showToast('Успешно установлено! Ссылка на панель — в «Логах».', 'success', 10000);

      const servers = JSON.parse(localStorage.getItem('selfhost_servers') || '[]');
      const s = servers.find(x => x.id === currentShServer.id);
      if (s) { s.installed = true; }
      localStorage.setItem('selfhost_servers', JSON.stringify(servers));
      renderSelfHostServers();
      closeModal('modal-sh-install');
    } catch (e) {
      showToast(`Ошибка установки Provider: ${e}`, 'error');
    }
    clearInterval(progressTimer);

    btn.disabled = false;
    btn.textContent = 'Запустить установку Provider';
  });

  $('#btn-sh-users-back').addEventListener('click', () => {
    goToPage('selfhost');
  });

  $('#btn-sh-add-user').addEventListener('click', async () => {
    if (!currentShServer) return;
    const username = $('#sh-new-user').value.trim();
    if (!username) return;
    
    const expiry = $('#sh-new-expiry').value || null;
    let limit_gb = parseInt($('#sh-new-limit').value);
    if (isNaN(limit_gb)) limit_gb = null;
    
    const btn = $('#btn-sh-add-user');
    btn.disabled = true;
    btn.textContent = '...';
    try {
      const pass = await Api.invoke('sh_add_user', { server: currentShServer, username, expiry, limit_gb });
      $('#sh-new-user').value = '';
      $('#sh-new-expiry').value = '';
      $('#sh-new-limit').value = '';
      
      saveShUserPass(currentShServer.id, username, pass);
      const link = `ssh://${username}:${pass}@${currentShServer.host}:${currentShServer.port}#${currentShServer.name}-${username}`;
      try {
        await navigator.clipboard.writeText(link);
        showToast('Пользователь создан! Ссылка скопирована в буфер', 'success');
      } catch (err) {
        prompt('Пользователь создан. Скопируйте ссылку:', link);
      }
      
      await loadShUsers();
    } catch (e) {
      showToast(`Ошибка: ${e}`, 'error');
    }
    btn.disabled = false;
    btn.textContent = 'Добавить';
  });

  renderSelfHostServers();
}

async function loadShUsers() {
  if (!currentShServer) return;
  const list = $('#sh-users-list');
  list.innerHTML = '<div style="text-align:center;padding:20px;color:var(--text-muted)">Загрузка...</div>';
  try {
    const users = await Api.invoke('sh_list_users', { server: currentShServer });
    list.innerHTML = '';
    if (users.length === 0) {
      list.innerHTML = '<div style="text-align:center;padding:20px;color:var(--text-muted)">Нет пользователей</div>';
      return;
    }
    for (const u of users) {
      const row = document.createElement('div');
      row.style.display = 'flex';
      row.style.justifyContent = 'space-between';
      row.style.alignItems = 'center';
      row.style.padding = '8px';
      row.style.borderBottom = '1px solid var(--border)';
      
      const savedPass = getShUserPass(currentShServer.id, u.username);
      
      row.innerHTML = `
        <div>
          <div style="font-weight: 500">${escHtml(u.username)}</div>
          <div style="font-size: 11px; color: var(--text-muted); margin-top: 2px;">
            ⏳ До: <span style="color:var(--text)">${escHtml(u.expiry)}</span> &nbsp;|&nbsp; 
            📊 Лимит: <span style="color:var(--text)">${u.limit_gb !== '--' ? escHtml(u.limit_gb) + ' ГБ' : '--'}</span>
          </div>
        </div>
        <div style="display:flex; gap: 8px;">
          ${savedPass 
            ? `<button class="btn btn-ghost btn-xs" data-copy="${u.username}" title="Скопировать ссылку ssh://">🔗 Конфиг</button>` 
            : ``}
          <button class="btn btn-ghost btn-xs" data-reset="${u.username}" title="Сбросить пароль и получить новую ссылку">🔑 Сброс пароля</button>
          <button class="btn btn-ghost btn-xs" data-del="${u.username}" style="color: #ff4d4f">Удалить</button>
        </div>
      `;
      
      if (savedPass) {
        row.querySelector(`[data-copy="${u.username}"]`).addEventListener('click', async () => {
          const link = `ssh://${u.username}:${savedPass}@${currentShServer.host}:${currentShServer.port}#${currentShServer.name}-${u.username}`;
          try {
            await navigator.clipboard.writeText(link);
            showToast('Конфиг скопирован!', 'success');
          } catch(e) { prompt('Скопируйте ссылку:', link); }
        });
      }
      
      row.querySelector(`[data-reset="${u.username}"]`).addEventListener('click', async () => {
        if (!confirm(`Сбросить пароль пользователя ${u.username}?`)) return;
        try {
          const pass = await Api.invoke('sh_reset_password', { server: currentShServer, username: u.username });
          saveShUserPass(currentShServer.id, u.username, pass);
          const link = `ssh://${u.username}:${pass}@${currentShServer.host}:${currentShServer.port}#${currentShServer.name}-${u.username}`;
          try {
            await navigator.clipboard.writeText(link);
            showToast('Новый пароль установлен и ссылка скопирована в буфер', 'success');
          } catch(e) {
            prompt('Пароль сброшен. Скопируйте новую ссылку:', link);
          }
          loadShUsers(); // Refresh to show "Copy" button
        } catch(e) { showToast(`Ошибка: ${e}`, 'error'); }
      });
      
      row.querySelector(`[data-del="${u.username}"]`).addEventListener('click', async () => {
        if (!confirm(`Удалить пользователя ${u.username}?`)) return;
        try {
          await Api.invoke('sh_del_user', { server: currentShServer, username: u.username });
          removeShUserPass(currentShServer.id, u.username);
          showToast('Удалено', 'success');
          loadShUsers();
        } catch(e) { showToast(`Ошибка: ${e}`, 'error'); }
      });
      
      list.appendChild(row);
    }
  } catch (e) {
    list.innerHTML = `<div style="color:#ff4d4f; padding: 10px;">Ошибка: ${escHtml(e)}</div>`;
  }
}

function renderSelfHostServers() {
  const list = $('#selfhost-list');
  if (!list) return;

  const servers = JSON.parse(localStorage.getItem('selfhost_servers') || '[]');
  list.innerHTML = '';

  if (servers.length === 0) {
    list.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-muted)">Нет добавленных серверов. Нажмите «+ Добавить сервер».</div>';
    return;
  }

  for (const srv of servers) {
    const card = document.createElement('div');
    card.className = 'server-item';
    card.style.display = 'block';
    card.style.height = 'auto';
    card.style.padding = '16px';
    
    card.innerHTML = `
      <div style="display:flex; justify-content:space-between; align-items:flex-start; margin-bottom: 12px;">
        <div class="server-item-info">
          <div class="server-name" style="font-size: 16px; margin-bottom: 4px;">${escHtml(srv.name)}</div>
          <div class="server-addr" style="color: var(--text-muted);">${escHtml(srv.host)}:${escHtml(srv.port)}</div>
        </div>
        <div class="server-actions" style="position: relative;">
          <button class="icon-btn" title="Опции" style="font-size: 18px; line-height: 1;" data-sh-menu="${srv.id}">⋮</button>
          <div id="sh-menu-${srv.id}" class="card" style="display: none; position: absolute; right: 0; top: 100%; z-index: 10; padding: 4px; min-width: 160px; box-shadow: 0 4px 12px rgba(0,0,0,0.3); border: 1px solid var(--border);">
            <button class="btn btn-ghost" style="width: 100%; text-align: left; color: #ff4d4f;" data-sh-del="${srv.id}">Удалить из приложения</button>
          </div>
        </div>
      </div>
      
      <div style="display:grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-bottom: 16px; color: var(--text-muted); font-size: 13px;">
        <div style="background: var(--bg-hover); padding: 8px; border-radius: 6px; text-align: center;">
          <div style="font-size: 11px; text-transform: uppercase; margin-bottom: 2px;">CPU</div>
          <div style="font-weight: 500; color: var(--text)" data-stat-cpu="${srv.id}">-- %</div>
        </div>
        <div style="background: var(--bg-hover); padding: 8px; border-radius: 6px; text-align: center;">
          <div style="font-size: 11px; text-transform: uppercase; margin-bottom: 2px;">RAM</div>
          <div style="font-weight: 500; color: var(--text)" data-stat-ram="${srv.id}">-- / --</div>
        </div>
      </div>

      <div style="display:flex; gap: 8px;">
        ${srv.installed 
          ? `<button class="btn btn-primary" style="flex: 1;" data-sh-users="${srv.id}">Пользователи</button>` 
          : `<button class="btn btn-primary" style="flex: 1;" data-sh-install="${srv.id}">Установить</button>`
        }
        <button class="btn btn-ghost" style="flex: 1;" data-sh-clean="${srv.id}">Очистить</button>
      </div>
    `;

    if (srv.installed) {
      Api.invoke('sh_get_stats', { server: srv })
        .then(stats => {
           const cpu = card.querySelector(`[data-stat-cpu="${srv.id}"]`);
           const ram = card.querySelector(`[data-stat-ram="${srv.id}"]`);
           if (cpu) cpu.textContent = stats.cpu;
           if (ram) ram.textContent = stats.ram;
        }).catch(() => {});
    }

    const menu = card.querySelector(`#sh-menu-${srv.id}`);
    card.querySelector(`[data-sh-menu="${srv.id}"]`).addEventListener('click', (e) => {
      e.stopPropagation();
      const isVisible = menu.style.display === 'block';
      document.querySelectorAll('[id^="sh-menu-"]').forEach(m => m.style.display = 'none');
      if (!isVisible) {
        menu.style.display = 'block';
      }
    });

    document.addEventListener('click', (e) => {
      if (!card.querySelector('.server-actions').contains(e.target)) {
        menu.style.display = 'none';
      }
    });

    card.querySelector(`[data-sh-del="${srv.id}"]`).addEventListener('click', (e) => {
      e.stopPropagation();
      if (!confirm(`Удалить Self-Host сервер "${srv.name}"?`)) return;
      const updated = servers.filter(s => s.id !== srv.id);
      localStorage.setItem('selfhost_servers', JSON.stringify(updated));
      renderSelfHostServers();
    });

    if (srv.installed) {
      card.querySelector(`[data-sh-users="${srv.id}"]`).addEventListener('click', (e) => {
        e.stopPropagation();
        currentShServer = srv;
        $('#sh-users-title').textContent = `Пользователи: ${srv.name}`;
        goToPage('sh-users');
        loadShUsers();
      });
    } else {
      card.querySelector(`[data-sh-install="${srv.id}"]`).addEventListener('click', (e) => {
        e.stopPropagation();
        currentShServer = srv;
        openModal('modal-sh-install');
      });
    }

    card.querySelector(`[data-sh-clean="${srv.id}"]`).addEventListener('click', async (e) => {
      e.stopPropagation();
      if (!confirm(`Вы УВЕРЕНЫ что хотите очистить сервер "${srv.name}"? Это полностью удалит всех пользователей, настройки моста, сбросит iptables и sshd config, а также удалит панель Provider (контейнеры, базу и файлы), если она установлена.`)) return;
      
      const btn = e.target;
      btn.disabled = true;
      btn.textContent = 'Очистка...';
      try {
        await Api.invoke('sh_clean_host', { server: srv });
        srv.installed = false;
        localStorage.setItem('selfhost_servers', JSON.stringify(servers));
        renderSelfHostServers();
        showToast('Сервер полностью очищен', 'success');
      } catch (err) {
        showToast(`Ошибка очистки: ${err}`, 'error');
        btn.disabled = false;
        btn.textContent = 'Очистить';
      }
    });

    list.appendChild(card);
  }
}


// ── Rules page ────────────────────────────────────────────────────────────────
const RULE_HINTS = {
  ip:          'Например: 192.168.1.100',
  cidr:        'Например: 10.0.0.0/8 или 192.168.1.0/24',
  domain:      'Например: example.com (только этот домен, точный резолв)',
  domain_zone: 'Например: example.com (этот домен + ВСЕ поддомены *.example.com, отслеживается динамически)',
  app:         'Полный путь к .exe: C:\\Program Files\\App\\app.exe',
};
const RULE_LABELS = { ip:'IP', cidr:'CIDR', domain:'Домен', domain_zone:'Домен. зона', app:'Прилож.' };
const RULE_PLACEHOLDERS = {
  ip:          '192.168.1.100',
  cidr:        '10.0.0.0/8',
  domain:      'example.com',
  domain_zone: 'example.com',
  app:         'C:\\Program Files\\App\\app.exe',
};

// The engine only steers traffic when the tunnel is up AND the toggle is on,
// so the status line reports both facts rather than just the setting.
async function refreshSplitStatus() {
  try {
    state.splitStatus = await Api.getSplitStatus();
  } catch (_) { return; }
  const s = state.splitStatus;

  const dot  = $('#split-dot');
  const text = $('#split-status-text');
  if (dot && text) {
    dot.className = 'split-dot ' + (s.active ? 'on' : s.enabled ? 'idle' : 'off');
    if (!s.enabled) {
      text.textContent = 'Правила выключены';
    } else if (!s.active) {
      text.textContent = `Правил: ${s.enabled_rules} из ${s.total_rules} — применятся при подключении`;
    } else {
      text.textContent =
        `Активно · ${s.mode === 'proxy' ? 'Proxy' : 'Bypass'} · ` +
        `правил ${s.enabled_rules}/${s.total_rules} · маршрутов ${s.route_count}`;
    }
  }

  const live = $('#split-live-info');
  if (live) {
    live.textContent = s.active
      ? `Активно · маршрутов: ${s.route_count}`
      : (s.enabled && s.total_rules === 0 ? 'Список правил пуст' : '');
  }
}

function visibleRules() {
  const rules = state.settings?.rules || [];
  const q = state.rulesSearch.toLowerCase();
  return rules.filter(r =>
    (state.rulesFilter === 'all' || r.rule_type === state.rulesFilter) &&
    (!q || String(r.value).toLowerCase().includes(q))
  );
}

function renderRules() {
  const list = $('#rules-list');
  const empty = $('#rules-empty');
  if (!list || !state.settings) return;

  const mode = state.settings.split_mode || 'bypass';
  $$('[data-mode]').forEach(b => b.classList.toggle('active', b.dataset.mode === mode));
  $$('[data-filter]').forEach(b => b.classList.toggle('active', b.dataset.filter === state.rulesFilter));

  const rules = visibleRules();
  list.innerHTML = '';

  if (rules.length === 0) {
    empty.classList.remove('hidden');
    const total = (state.settings.rules || []).length;
    empty.querySelector('div:last-child').textContent =
      total === 0 ? 'Правила не добавлены' : 'Ничего не найдено по фильтру';
    refreshSplitStatus();
    return;
  }
  empty.classList.add('hidden');

  for (const rule of rules) {
    const row = document.createElement('div');
    row.className = 'rule-item' + (rule.enabled ? '' : ' rule-off');
    row.innerHTML = `
      <span class="rule-type-badge type-${rule.rule_type}">${RULE_LABELS[rule.rule_type] || rule.rule_type}</span>
      <span class="rule-value" title="${escHtml(rule.value)}">${escHtml(rule.value)}</span>
      <label class="toggle">
        <input type="checkbox" data-rule-toggle="${rule.id}" ${rule.enabled ? 'checked' : ''} />
        <span class="toggle-slider"></span>
      </label>
      <div class="rule-actions">
        <button class="icon-btn" data-rule-edit="${rule.id}" title="Изменить">✎</button>
        <button class="icon-btn" data-rule-del="${rule.id}" title="Удалить">🗑</button>
      </div>
    `;

    row.querySelector(`[data-rule-toggle="${rule.id}"]`).addEventListener('change', async (e) => {
      const on = e.target.checked;
      try {
        await Api.toggleRule(rule.id, on);
        state.settings = await Api.getSettings();
        renderRules();
      } catch (err) {
        e.target.checked = !on;
        showToast(`Ошибка: ${err}`, 'error');
      }
    });

    row.querySelector(`[data-rule-edit="${rule.id}"]`).addEventListener('click', () => openRuleModal(rule));

    row.querySelector(`[data-rule-del="${rule.id}"]`).addEventListener('click', async () => {
      if (!confirm(`Удалить правило «${rule.value}»?`)) return;
      try {
        await Api.deleteRule(rule.id);
        state.settings = await Api.getSettings();
        renderRules();
        showToast('Правило удалено', 'success');
      } catch (err) { showToast(`Ошибка: ${err}`, 'error'); }
    });

    list.appendChild(row);
  }

  refreshSplitStatus();
}

function openRuleModal(rule = null) {
  state.editingRuleId = rule ? rule.id : null;
  $('#rule-type').value  = rule ? rule.rule_type : 'ip';
  $('#rule-value').value = rule ? rule.value : '';
  $('#modal-rule .modal-header h3').textContent = rule ? 'Изменить правило' : 'Добавить правило';
  $('#btn-save-rule').textContent = rule ? 'Сохранить' : 'Добавить';
  updateRuleHint();
  openModal('modal-rule');
  setTimeout(() => $('#rule-value').focus(), 50);
}

function initRulesPage() {
  // Mode tabs
  $$('[data-mode]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const mode = btn.dataset.mode;
      try {
        await Api.setSplitMode(mode);
        state.settings.split_mode = mode;
        renderRules();
        updateSplitModeDesc();
        showToast(mode === 'proxy'
          ? 'Режим Proxy — через VPN идёт только список'
          : 'Режим Bypass — через VPN идёт всё, кроме списка', 'success');
      } catch (err) { showToast(`Ошибка: ${err}`, 'error'); }
    });
  });

  // Type filter chips
  $$('[data-filter]').forEach(chip => {
    chip.addEventListener('click', () => {
      state.rulesFilter = chip.dataset.filter;
      renderRules();
    });
  });

  $('#rules-search')?.addEventListener('input', (e) => {
    state.rulesSearch = e.target.value.trim();
    renderRules();
  });

  $('#btn-add-rule').addEventListener('click', () => openRuleModal(null));
  $('#rule-type').addEventListener('change', updateRuleHint);

  $('#rule-value').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') $('#btn-save-rule').click();
  });

  $('#btn-save-rule').addEventListener('click', async () => {
    const ruleType = $('#rule-type').value;
    const value    = $('#rule-value').value.trim();
    if (!value) { showToast('Укажите значение правила', 'error'); return; }

    const editing = state.editingRuleId;
    const existing = editing
      ? (state.settings.rules || []).find(r => r.id === editing)
      : null;
    const rule = {
      id: editing || crypto.randomUUID(),
      rule_type: ruleType,
      value,
      enabled: existing ? existing.enabled : true,
      note: existing ? (existing.note ?? '') : '',
    };

    try {
      // Validation lives in Rust, so the error text shown here is the backend's.
      if (editing) await Api.updateRule(rule);
      else         await Api.addRule(rule);
      state.settings = await Api.getSettings();
      closeModal('modal-rule');
      state.editingRuleId = null;
      renderRules();
      showToast(editing ? 'Правило изменено' : 'Правило добавлено', 'success');
    } catch(e) { showToast(`${e}`, 'error'); }
  });

  initAppPicker();
  initRulesIo();
}

// ── App picker ────────────────────────────────────────────────────────────────
function initAppPicker() {
  $('#btn-pick-app')?.addEventListener('click', async () => {
    openModal('modal-apps');
    $('#app-search').value = '';
    await loadRunningApps();
  });

  $('#btn-apps-refresh')?.addEventListener('click', loadRunningApps);
  $('#app-search')?.addEventListener('input', renderAppList);
}

async function loadRunningApps() {
  const box = $('#app-list');
  box.innerHTML = '<div class="app-list-empty">Загрузка списка процессов…</div>';
  try {
    state.runningApps = await Api.listRunningApps();
  } catch (e) {
    box.innerHTML = `<div class="app-list-empty">Не удалось получить список: ${escHtml(e)}</div>`;
    return;
  }
  renderAppList();
}

function renderAppList() {
  const box = $('#app-list');
  if (!box) return;
  const q = ($('#app-search')?.value || '').toLowerCase();
  const existing = new Set(
    (state.settings?.rules || [])
      .filter(r => r.rule_type === 'app')
      .map(r => String(r.value).toLowerCase())
  );

  const apps = state.runningApps.filter(a =>
    !q || a.name.toLowerCase().includes(q) || a.path.toLowerCase().includes(q)
  );

  if (apps.length === 0) {
    box.innerHTML = '<div class="app-list-empty">Ничего не найдено</div>';
    return;
  }

  box.innerHTML = '';
  for (const app of apps) {
    const added = existing.has(app.path.toLowerCase());
    const row = document.createElement('div');
    row.className = 'app-item' + (added ? ' added' : '');
    row.innerHTML = `
      <div class="app-item-info">
        <div class="app-item-name">${escHtml(app.name)}</div>
        <div class="app-item-path" title="${escHtml(app.path)}">${escHtml(app.path)}</div>
      </div>
      <button class="btn btn-ghost btn-xs" ${added ? 'disabled' : ''}>${added ? '✓ В списке' : '+ Добавить'}</button>
    `;
    if (!added) {
      row.querySelector('button').addEventListener('click', async () => {
        try {
          await Api.addRule({
            id: crypto.randomUUID(),
            rule_type: 'app',
            value: app.path,
            enabled: true,
            note: '',
          });
          state.settings = await Api.getSettings();
          renderRules();
          renderAppList();
          showToast(`Добавлено: ${app.name}`, 'success');
        } catch (e) { showToast(`${e}`, 'error'); }
      });
    }
    box.appendChild(row);
  }
}

// ── Rules import / export ─────────────────────────────────────────────────────
function initRulesIo() {
  $('#btn-io-rules')?.addEventListener('click', async () => {
    try {
      $('#io-text').value = await Api.exportRules();
    } catch (e) {
      $('#io-text').value = '';
      showToast(`Ошибка экспорта: ${e}`, 'error');
    }
    $('#io-replace').checked = false;
    openModal('modal-io');
  });

  $('#btn-io-copy')?.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText($('#io-text').value);
      showToast('Скопировано в буфер обмена', 'success');
    } catch (_) {
      $('#io-text').select();
      showToast('Выделено — скопируйте вручную (Ctrl+C)');
    }
  });

  $('#btn-io-import')?.addEventListener('click', async () => {
    const text = $('#io-text').value;
    const replace = $('#io-replace').checked;
    if (replace && !confirm('Текущий список правил будет удалён. Продолжить?')) return;
    try {
      const added = await Api.importRules(text, replace);
      state.settings = await Api.getSettings();
      closeModal('modal-io');
      renderRules();
      showToast(added > 0 ? `Импортировано правил: ${added}` : 'Новых правил нет — все уже в списке',
                added > 0 ? 'success' : 'info');
    } catch (e) { showToast(`${e}`, 'error'); }
  });
}

function updateRuleHint() {
  const type = $('#rule-type').value;
  $('#rule-hint').textContent = RULE_HINTS[type] || '';
  $('#rule-value-label').textContent = RULE_LABELS[type] || 'Значение';
  $('#rule-value').placeholder = RULE_PLACEHOLDERS[type] || '';
}

function updateSplitModeDesc() {
  const mode = state.settings?.split_mode;
  $('#split-mode-desc').textContent =
    mode === 'proxy'
      ? 'Proxy — только список через VPN'
      : 'Bypass — всё через VPN, кроме списка';
}

// ── Logs page ─────────────────────────────────────────────────────────────────
let logs = [];

function appendLog(msg) {
  const max = state.settings?.log_max_lines || 500;
  logs.push({ time: new Date().toLocaleTimeString(), msg });
  if (logs.length > max) logs.shift();
  renderLogs();
  scrollLogsBottom();
}

function renderLogs() {
  const container = $('#log-container');
  if (!container) return;
  if (logs.length === 0) {
    container.innerHTML = '<div class="log-empty">Логи пусты…</div>';
    return;
  }
  container.innerHTML = logs.map(e => {
    const cls = e.msg.includes('ERROR') || e.msg.includes('error')
      ? 'error' : e.msg.includes('WARN') ? 'warn' : 'info';
    return `<div class="log-entry ${cls}">[${e.time}] ${escHtml(e.msg)}</div>`;
  }).join('');
}

function scrollLogsBottom() {
  const c = $('#log-container');
  if (c) c.scrollTop = c.scrollHeight;
}

function initLogsPage() {
  $('#btn-clear-logs').addEventListener('click', () => {
    logs = [];
    renderLogs();
  });
}

// Subscribe to VPN log events from Rust backend
async function subscribeToLogs() {
  try {
    const { listen } = window.__TAURI__.event;
    await listen('vpn-log', (event) => {
      appendLog(event.payload);
    });
  } catch (_) { /* Tauri event API not available in dev */ }
}

// ── Settings page ─────────────────────────────────────────────────────────────
function initSettingsPage() {
  $('#btn-save-settings').addEventListener('click', async () => {
    const theme = $('#setting-theme').value;
    state.settings.theme            = theme;
    state.settings.auto_reconnect   = $('#setting-autoreconnect').checked;
    state.settings.minimize_to_tray = $('#setting-tray').checked;
    state.settings.mtu              = parseInt($('#setting-mtu').value) || 1400;
    const dnsRaw = $('#setting-dns').value;
    state.settings.dns_servers = dnsRaw.split(',').map(d => d.trim()).filter(Boolean);

    try {
      await Api.updateSettings(state.settings);
      applyTheme(theme);
      showToast('Настройки сохранены', 'success');
    } catch(e) { showToast(`Ошибка: ${e}`, 'error'); }
  });

  // Picking a theme applies and persists it immediately — with 14 of them,
  // requiring a trip to «Сохранить» after each try is needless friction.
  // update_theme writes only that field, so unsaved edits elsewhere on the
  // form are left alone.
  $('#setting-theme').addEventListener('change', async (e) => {
    const theme = e.target.value;
    applyTheme(theme);
    if (state.settings) state.settings.theme = theme;
    try { await Api.updateTheme(theme); }
    catch (err) { showToast(`Не удалось сохранить тему: ${err}`, 'error'); }
  });
}

function loadSettingsUI() {
  if (!state.settings) return;
  const s = state.settings;
  $('#setting-theme').value           = s.theme || 'dark';
  $('#setting-autoreconnect').checked = s.auto_reconnect ?? true;
  $('#setting-tray').checked          = s.minimize_to_tray ?? true;
  $('#setting-mtu').value             = s.mtu || 1400;
  $('#setting-dns').value             = (s.dns_servers || []).join(', ');
  $('#toggle-split').checked          = s.split_enabled ?? false;
  applyTheme(s.theme || 'dark');
  updateSplitModeDesc();
  refreshSplitStatus();
}

// ── Escape HTML ───────────────────────────────────────────────────────────────
function escHtml(s) {
  return String(s)
    .replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;')
    .replace(/"/g,'&quot;').replace(/'/g,'&#39;');
}

// ── Bootstrap ─────────────────────────────────────────────────────────────────
async function init() {
  initTitlebar();

  // Nav
  $$('.nav-item').forEach(n => {
    n.addEventListener('click', () => goToPage(n.dataset.page));
  });

  // Close modals on Escape
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') $$('.modal:not(.hidden)').forEach(m => m.classList.add('hidden'));
  });

  // Load data
  try {
    state.settings = await Api.getSettings();
    state.servers  = await Api.getServers();
    state.vpnState = await Api.vpnGetState();
    state.selectedServerId = state.settings.selected_server_id || null;
  } catch (e) {
    console.warn('Failed to load state from Tauri (running in browser?):', e);
    state.settings = {
      theme: 'dark', split_enabled: false, split_mode: 'bypass',
      rules: [], auto_reconnect: true, minimize_to_tray: true,
      mtu: 1400, dns_servers: ['8.8.8.8','1.1.1.1'],
      selected_server_id: null, log_max_lines: 500,
    };
    state.servers = { servers: [], subscriptions: [] };
    state.vpnState = { status: 'disconnected', uptime_secs:0, tx_bytes:0, rx_bytes:0 };
  }

  loadSettingsUI();
  updateSelectedServerInfo();

  // Pages
  initDashboard();
  initServersPage();
  initSelfHostPage();
  initRulesPage();
  initLogsPage();
  initSettingsPage();

  // Initial VPN state sync
  if (state.vpnState?.status === 'connected') {
    setConnectedUI();
    $('#stats-row').style.display = 'flex';
  } else {
    setDisconnectedUI();
  }

  // Subscribe to log events from Rust
  await subscribeToLogs();

  // Start polling VPN state every second
  startPolling();

  // Default page
  goToPage('dashboard');
}

document.addEventListener('DOMContentLoaded', init);
