#!/usr/bin/env bash
#
# ForgeFox VPN — аудит сервера. ТОЛЬКО ЧТЕНИЕ, ничего не меняет.
# Запуск: ssh root@server 'bash -s' < server/remote-audit.sh
#
set -uo pipefail

RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; DIM=$'\033[2m'; RST=$'\033[0m'
ok()   { echo "  ${GRN}✓${RST} $*"; }
info() { echo "  ${DIM}·${RST} $*"; }
warn() { echo "  ${YLW}!${RST} $*"; }
err()  { echo "  ${RED}✗${RST} $*"; }
hdr()  { echo; echo "── $* ──"; }

echo "═════════ АУДИТ $(hostname) ═════════"

hdr "1. Система"
info "$(. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME") / ядро $(uname -r)"
info "аптайм: $(uptime -p 2>/dev/null || uptime)"

hdr "2. Мост"
if [ -x /usr/local/bin/forgefox-bridge ]; then
  ok "forgefox-bridge есть ($(stat -c '%s байт, %y' /usr/local/bin/forgefox-bridge | cut -d. -f1))"
else
  err "forgefox-bridge ОТСУТСТВУЕТ — клиент откатывается на Python-мост"
fi
command -v gcc >/dev/null && info "gcc: $(gcc --version | head -1)" || warn "gcc не установлен"
# PATH неинтерактивного SSH — именно в нём клиент ищет мост через command -v
info "PATH при exec без tty: $(ssh_path=$(env -i sh -lc 'echo $PATH' 2>/dev/null); echo "${ssh_path:-?}")"

hdr "3. Сетевые интерфейсы"
ip -br addr show 2>/dev/null | sed 's/^/      /'
DEFDEV=$(ip route show default | awk '{for(i=1;i<NF;i++) if($i=="dev") print $(i+1); exit}')
DEFCNT=$(ip route show default | wc -l)
info "интерфейс по умолчанию: ${DEFDEV:-НЕ НАЙДЕН} (маршрутов по умолчанию: $DEFCNT)"
[ "$DEFCNT" -gt 1 ] && warn "несколько default-маршрутов — MASQUERADE мог сесть не на тот интерфейс"

# Пересечение своей адресации с туннельной подсетью 10.x
OWN10=$(ip -4 addr show | awk '/inet /{print $2}' | grep -E '^10\.' || true)
if [ -n "$OWN10" ]; then
  err "у сервера СВОЙ адрес в 10.0.0.0/8: $(echo "$OWN10" | tr '\n' ' ')"
  err "  это тот же диапазон, что у туннеля — маршруты и NAT конфликтуют"
else
  ok "адресов сервера в 10.0.0.0/8 нет — конфликта с туннелем нет"
fi

R10=$(ip route show | grep -E '^10\.|10\.0\.0\.0/8' || true)
[ -n "$R10" ] && { warn "маршруты в 10/8 уже есть:"; echo "$R10" | sed 's/^/      /'; }

hdr "4. Форвардинг"
FWD=$(sysctl -n net.ipv4.ip_forward 2>/dev/null)
[ "$FWD" = 1 ] && ok "ip_forward=1" || err "ip_forward=$FWD — сервер не маршрутизирует"
info "cc/qdisc: $(sysctl -n net.ipv4.tcp_congestion_control 2>/dev/null)/$(sysctl -n net.core.default_qdisc 2>/dev/null)"
[ -f /etc/sysctl.d/99-forgefox-vpn.conf ] && ok "конфиг install.sh на месте" || err "нет /etc/sysctl.d/99-forgefox-vpn.conf — install.sh здесь не отработал"
# rp_filter=1 роняет пакеты из туннеля, если обратный маршрут не через тот же iface
for f in /proc/sys/net/ipv4/conf/all/rp_filter /proc/sys/net/ipv4/conf/default/rp_filter; do
  v=$(cat "$f" 2>/dev/null)
  [ "$v" = 1 ] && warn "$(basename $(dirname $f))/rp_filter=1 — строгая обратная проверка может резать туннель" \
               || info "$(basename $(dirname $f))/rp_filter=$v"
done

hdr "5. iptables — политики и правила"
info "бэкенд: $(iptables --version 2>/dev/null)"
echo "    policy filter:"
iptables -S 2>/dev/null | grep '^-P' | sed 's/^/      /'
FWDPOL=$(iptables -S 2>/dev/null | awk '/^-P FORWARD/{print $3}')
[ "$FWDPOL" = DROP ] && warn "политика FORWARD = DROP (обычно ставит Docker) — спасают только ACCEPT-правила выше" \
                     || ok "политика FORWARD = ${FWDPOL:-?}"

echo "    FORWARD:"
iptables -S FORWARD 2>/dev/null | sed 's/^/      /'
echo "    INPUT:"
iptables -S INPUT 2>/dev/null | head -25 | sed 's/^/      /'
echo "    nat/POSTROUTING:"
iptables -t nat -S POSTROUTING 2>/dev/null | sed 's/^/      /'
echo "    mangle/FORWARD:"
iptables -t mangle -S FORWARD 2>/dev/null | sed 's/^/      /'

NATC=$(iptables -t nat -S POSTROUTING 2>/dev/null | grep -c MASQUERADE || true)
[ "${NATC:-0}" -ge 1 ] && ok "MASQUERADE-правил: $NATC" || err "MASQUERADE НЕТ — ответы из интернета не вернутся клиенту"
MSSC=$(iptables -t mangle -S FORWARD 2>/dev/null | grep -c TCPMSS || true)
[ "${MSSC:-0}" -ge 1 ] && ok "клампинг MSS есть" || warn "клампинга MSS нет — мелкое ходит, TLS виснет"

hdr "6. nftables (может действовать параллельно с iptables)"
if command -v nft >/dev/null 2>&1; then
  NFT=$(nft list ruleset 2>/dev/null | grep -vE '^\s*$' || true)
  NFTLINES=$(echo "$NFT" | wc -l)
  if [ "$NFTLINES" -gt 1 ]; then
    NFTDROP=$(echo "$NFT" | grep -cE 'policy drop' || true)
    [ "${NFTDROP:-0}" -gt 0 ] && err "в nftables есть цепочки с policy drop — они режут трафик мимо iptables" \
                              || info "nft-правила есть, drop-политик не видно ($NFTLINES строк)"
    echo "$NFT" | grep -E 'chain|policy|drop|reject|masquerade' | head -30 | sed 's/^/      /'
  else
    ok "nft-ruleset пуст"
  fi
else
  info "nft не установлен"
fi

hdr "7. Файрволы поверх"
command -v ufw >/dev/null && { S=$(ufw status 2>/dev/null | head -1); [ "${S#*active}" != "$S" ] && warn "ufw: $S — может резать FORWARD" || info "ufw: $S"; } || info "ufw нет"
systemctl is-active firewalld 2>/dev/null | grep -q '^active' && warn "firewalld активен — свои зоны и политики" || info "firewalld не активен"
command -v docker >/dev/null && warn "docker установлен — он переписывает FORWARD и ставит policy DROP" || info "docker нет"

hdr "8. TUN"
[ -c /dev/net/tun ] && ok "/dev/net/tun есть ($(stat -c '%A' /dev/net/tun))" || err "/dev/net/tun ОТСУТСТВУЕТ — мост не сможет открыть туннель"
lsmod 2>/dev/null | grep -q '^tun' && ok "модуль tun загружен" || warn "модуль tun не в lsmod (может быть вкомпилен в ядро)"
TUNS=$(ip -br addr show type tun 2>/dev/null || true)
[ -n "$TUNS" ] && { warn "уже есть TUN-интерфейсы (остатки прошлых сессий?):"; echo "$TUNS" | sed 's/^/      /'; } || ok "висящих TUN нет"

hdr "9. Живые процессы моста"
P=$(pgrep -a -f 'forgefox-bridge|dev/net/tun' 2>/dev/null | grep -v 'bash -s' || true)
[ -n "$P" ] && { echo "$P" | sed 's/^/      /'; warn "мост уже запущен — если клиент не подключён, это зависший процесс"; } || ok "запущенных мостов нет"

hdr "10. sshd"
SSHDB=$(command -v sshd || echo /usr/sbin/sshd)
if [ -x "$SSHDB" ]; then
  "$SSHDB" -T 2>/dev/null | grep -iE '^(permitrootlogin|permittunnel|compression|ciphers|clientaliveinterval|usedns|allowtcpforwarding|permitopen)' | sed 's/^/      /'
else
  err "sshd не найден"
fi
[ -f /etc/ssh/sshd_config.d/00-forgefox-vpn.conf ] && ok "drop-in install.sh на месте" || warn "drop-in install.sh отсутствует"

hdr "11. Связность самого сервера"
for t in 1.1.1.1 8.8.8.8; do
  ping -c1 -W2 "$t" >/dev/null 2>&1 && ok "ping $t" || err "ping $t не проходит"
done
C=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 https://2ip.ru 2>/dev/null)
[ "${C:0:1}" = 2 ] || [ "${C:0:1}" = 3 ] && ok "https://2ip.ru → HTTP $C" || err "https://2ip.ru → «$C» — сервер сам не открывает сайт"
info "внешний IP сервера: $(curl -s --max-time 10 https://api.ipify.org 2>/dev/null || echo '?')"

echo
echo "═════════ КОНЕЦ ═════════"
