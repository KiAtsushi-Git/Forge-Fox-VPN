#!/usr/bin/env bash
#
# ForgeFox VPN — уборка мусора, накопившегося на сервере.
#
# Без аргументов НИЧЕГО не меняет — только показывает, что нашёл.
# Меняет только по явному флагу:
#   --prune-nat      снять MASQUERADE-правила для подсетей, у которых
#                    уже нет живого TUN-интерфейса
#   --kill-bridges   прибить зависшие процессы моста (СНАЧАЛА отключите клиента!)
#   --all            и то, и другое
#
set -uo pipefail

RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; DIM=$'\033[2m'; RST=$'\033[0m'
ok()   { echo "  ${GRN}✓${RST} $*"; }
info() { echo "  ${DIM}·${RST} $*"; }
warn() { echo "  ${YLW}!${RST} $*"; }
err()  { echo "  ${RED}✗${RST} $*"; }

PRUNE=0; KILL=0
for a in "$@"; do
  case "$a" in
    --prune-nat)    PRUNE=1 ;;
    --kill-bridges) KILL=1 ;;
    --all)          PRUNE=1; KILL=1 ;;
    *) err "неизвестный аргумент: $a"; exit 1 ;;
  esac
done

[ "$(id -u)" -ne 0 ] && { err "нужен root"; exit 1; }

echo
echo "══════════════════════════════════════════════"
echo " ForgeFox VPN — уборка"
[ "$PRUNE$KILL" = "00" ] && echo " (режим просмотра — ничего не меняется)"
echo "══════════════════════════════════════════════"

# ── 1. Зависшие мосты ─────────────────────────────────────────────────────────
echo
echo "[1] Процессы моста"

# Питоновский фоллбэк опознаём по ioctl-константе TUNSETIFF в командной строке,
# а не по слову python: под этим же именем крутится половина системы.
mapfile -t PIDS < <(pgrep -f 'forgefox-bridge' 2>/dev/null; pgrep -f '0x400454ca' 2>/dev/null)
# уникализируем и выкидываем себя
SELF=$$
UNIQ=()
for p in "${PIDS[@]:-}"; do
  [ -z "$p" ] && continue
  [ "$p" = "$SELF" ] && continue
  case " ${UNIQ[*]:-} " in *" $p "*) continue ;; esac
  UNIQ+=("$p")
done

if [ "${#UNIQ[@]}" -eq 0 ]; then
  ok "зависших мостов нет"
else
  for p in "${UNIQ[@]}"; do
    START=$(ps -o lstart= -p "$p" 2>/dev/null | sed 's/^ *//')
    ETIME=$(ps -o etime= -p "$p" 2>/dev/null | tr -d ' ')
    KIND=$(grep -q forgefox-bridge /proc/"$p"/cmdline 2>/dev/null && echo "C" || echo "Python")
    # Родитель-sshd жив? Если нет — сессия точно брошена.
    PPID_=$(ps -o ppid= -p "$p" 2>/dev/null | tr -d ' ')
    PCMD=$(ps -o comm= -p "${PPID_:-0}" 2>/dev/null | tr -d ' ')
    info "pid $p  мост=$KIND  живёт $ETIME  (с $START)  родитель=${PCMD:-нет}"
  done
  warn "${#UNIQ[@]} шт. Каждый держит свой TUN и свою SSH-сессию."
  if [ "$KILL" = 1 ]; then
    for p in "${UNIQ[@]}"; do kill -TERM "$p" 2>/dev/null; done
    sleep 2
    for p in "${UNIQ[@]}"; do kill -0 "$p" 2>/dev/null && kill -KILL "$p" 2>/dev/null; done
    sleep 1
    LEFT=0
    for p in "${UNIQ[@]}"; do kill -0 "$p" 2>/dev/null && LEFT=$((LEFT+1)); done
    [ "$LEFT" -eq 0 ] && ok "все прибиты, TUN-интерфейсы уйдут сами" \
                      || err "осталось живых: $LEFT"
  else
    info "снять: $0 --kill-bridges  (сначала отключите клиента)"
  fi
fi

# ── 2. Осиротевшие MASQUERADE-правила ─────────────────────────────────────────
echo
echo "[2] Правила NAT для мёртвых подсетей"

# Живые туннельные подсети = то, что реально висит на интерфейсах
LIVE=$(ip -4 -o addr show 2>/dev/null | awk '{print $4}' \
       | grep -E '^10\.' | cut -d/ -f1 \
       | awk -F. '{print $1"."$2"."$3".0/24"}' | sort -u)

# Кандидаты: правила ровно на /24 внутри 10/8. Правило на весь 10.0.0.0/8
# ставит install.sh — оно и должно остаться, его не трогаем.
mapfile -t ORPH < <(iptables -t nat -S POSTROUTING 2>/dev/null \
  | grep -E '^-A POSTROUTING -s 10\.[0-9]+\.[0-9]+\.0/24 ' | while read -r line; do
      net=$(echo "$line" | awk '{for(i=1;i<NF;i++) if($i=="-s") print $(i+1)}')
      echo "$LIVE" | grep -qx "$net" || echo "$line"
    done)

if [ "${#ORPH[@]}" -eq 0 ]; then
  ok "осиротевших правил нет"
else
  printf '      %s\n' "${ORPH[@]}"
  warn "${#ORPH[@]} шт. — мост добавляет правило на каждую сессию и не убирает"
  if [ "$PRUNE" = 1 ]; then
    N=0
    for line in "${ORPH[@]}"; do
      # -A -> -D, аргументы те же
      # shellcheck disable=SC2086
      iptables -t nat ${line/#-A/-D} 2>/dev/null && N=$((N+1))
    done
    ok "снято правил: $N"
  else
    info "снять: $0 --prune-nat"
  fi
fi

# ── 3. FORWARD-правила на несуществующие интерфейсы ────────────────────────────
echo
echo "[3] FORWARD на исчезнувшие интерфейсы"
mapfile -t DEADF < <(iptables -S FORWARD 2>/dev/null \
  | grep -E '^-A FORWARD .*-[io] tun[0-9]+' | while read -r line; do
      dev=$(echo "$line" | grep -oE 'tun[0-9]+' | head -1)
      [ -d "/sys/class/net/$dev" ] || echo "$line"
    done)

if [ "${#DEADF[@]}" -eq 0 ]; then
  ok "мёртвых FORWARD-правил нет"
else
  printf '      %s\n' "${DEADF[@]}"
  if [ "$PRUNE" = 1 ]; then
    N=0
    for line in "${DEADF[@]}"; do
      # shellcheck disable=SC2086
      iptables ${line/#-A/-D} 2>/dev/null && N=$((N+1))
    done
    ok "снято правил: $N"
  else
    info "снять: $0 --prune-nat"
  fi
fi

# ── 4. Сохранение ─────────────────────────────────────────────────────────────
if [ "$PRUNE" = 1 ]; then
  echo
  if command -v netfilter-persistent >/dev/null 2>&1; then
    netfilter-persistent save >/dev/null 2>&1 && ok "правила сохранены"
  elif command -v iptables-save >/dev/null 2>&1 && [ -d /etc/iptables ]; then
    iptables-save > /etc/iptables/rules.v4 && ok "правила сохранены"
  fi
fi

echo
echo "══════════════════════════════════════════════"
echo
