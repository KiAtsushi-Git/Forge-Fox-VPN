#!/usr/bin/env bash
#
# ForgeFox VPN — диагностика серверной стороны.
# Запускать во время активного подключения клиента: ./diag.sh
#
# Отвечает на один вопрос: доходит ли трафик клиента до сервера и уходит ли
# он дальше в интернет. Всё остальное — детали для чтения глазами.
#
set -uo pipefail

RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; DIM=$'\033[2m'; RST=$'\033[0m'
ok()   { echo "  ${GRN}✓${RST} $*"; }
info() { echo "  ${DIM}·${RST} $*"; }
warn() { echo "  ${YLW}!${RST} $*"; }
err()  { echo "  ${RED}✗${RST} $*"; }

[ "$(id -u)" -ne 0 ] && { err "нужен root: sudo ./diag.sh"; exit 1; }

echo
echo "══════════════════════════════════════════════"
echo " ForgeFox VPN — диагностика"
echo "══════════════════════════════════════════════"

# ── 1. Живёт ли мост ──────────────────────────────────────────────────────────
echo
echo "[1] Процесс моста"
BRIDGES=$(pgrep -a -f 'forgefox-bridge' 2>/dev/null | grep -v diag || true)
PY=$(pgrep -a -f "dev/net/tun" 2>/dev/null | grep python || true)

if [ -n "$BRIDGES" ]; then
  echo "$BRIDGES" | sed 's/^/      /'
  N=$(echo "$BRIDGES" | wc -l)
  [ "$N" -eq 1 ] && ok "запущен ровно один мост (C)" \
                 || warn "мостов $N — лишние держат адрес на своём TUN и ломают обратный маршрут"
elif [ -n "$PY" ]; then
  echo "$PY" | sed 's/^/      /'
  warn "работает Python-мост, а не C — клиент не нашёл forgefox-bridge в PATH"
else
  err "мост не запущен — клиент не подключён либо процесс упал сразу после старта"
fi

# ── 2. TUN-интерфейсы ─────────────────────────────────────────────────────────
echo
echo "[2] TUN-интерфейсы"
TUNS=$(ip -br addr show type tun 2>/dev/null || true)
if [ -z "$TUNS" ]; then
  err "ни одного TUN — мост не поднял интерфейс (нет прав на /dev/net/tun?)"
else
  echo "$TUNS" | sed 's/^/      /'
  DUP=$(echo "$TUNS" | awk '{for(i=3;i<=NF;i++) print $i}' | grep -c '10\.' || true)
  [ "${DUP:-0}" -gt 1 ] && err "адрес туннеля висит на нескольких интерфейсах — обратные пакеты уйдут в мёртвый TUN" \
                        || ok "адрес туннеля ровно на одном интерфейсе"
fi

# ── 3. Форвардинг и NAT ───────────────────────────────────────────────────────
echo
echo "[3] Форвардинг и NAT"
FWD=$(sysctl -n net.ipv4.ip_forward 2>/dev/null)
[ "$FWD" = 1 ] && ok "ip_forward=1" || err "ip_forward=$FWD — сервер не маршрутизирует, туннель будет молчать"

NAT=$(iptables -t nat -S POSTROUTING | grep -c MASQUERADE || true)
[ "${NAT:-0}" -ge 1 ] && ok "правил MASQUERADE: $NAT" || err "нет MASQUERADE — ответы из интернета не вернутся"
iptables -t nat -S POSTROUTING | grep MASQUERADE | sed 's/^/      /'

MSS=$(iptables -t mangle -S FORWARD 2>/dev/null | grep -c TCPMSS || true)
[ "${MSS:-0}" -ge 1 ] && ok "MSS клампится" \
                      || warn "MSS не клампится — мелкое работает, TLS-handshake виснет; перезапустите install.sh"

# ── 4. Счётчики: доходит ли трафик ────────────────────────────────────────────
echo
echo "[4] Счётчики за 5 секунд"
DEV=$(ip -br addr show type tun 2>/dev/null | awk 'NR==1{print $1}')
if [ -z "$DEV" ]; then
  warn "нет TUN — считать нечего"
else
  R1=$(cat /sys/class/net/"$DEV"/statistics/rx_packets 2>/dev/null || echo 0)
  T1=$(cat /sys/class/net/"$DEV"/statistics/tx_packets 2>/dev/null || echo 0)
  info "зайдите на проблемный сайт в браузере — считаю 5 секунд…"
  sleep 5
  R2=$(cat /sys/class/net/"$DEV"/statistics/rx_packets 2>/dev/null || echo 0)
  T2=$(cat /sys/class/net/"$DEV"/statistics/tx_packets 2>/dev/null || echo 0)
  DR=$((R2-R1)); DT=$((T2-T1))
  info "$DEV: от клиента +$DR пакетов, к клиенту +$DT"

  if [ "$DR" -eq 0 ]; then
    err "от клиента НИЧЕГО не пришло — проблема на стороне клиента: маршрут в туннель не поставлен"
  elif [ "$DT" -eq 0 ]; then
    err "запросы приходят, ответов нет — сервер не выпускает трафик наружу (NAT/форвардинг/файрвол провайдера)"
  else
    ok "трафик идёт в обе стороны — туннель живой, ищите причину выше по стеку (MTU, DNS)"
  fi
fi

# ── 5. Может ли сам сервер наружу ─────────────────────────────────────────────
echo
echo "[5] Связность самого сервера"
if curl -s -o /dev/null -w '%{http_code}' --max-time 8 https://2ip.ru 2>/dev/null | grep -qE '^[23]'; then
  ok "сервер сам открывает 2ip.ru"
else
  err "сервер САМ не может открыть 2ip.ru — дело не в туннеле, а в сети сервера"
fi

echo
echo "══════════════════════════════════════════════"
echo
