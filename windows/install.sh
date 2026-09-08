#!/usr/bin/env bash
#
# ForgeFox VPN — настройка / донастройка сервера.
#
# Скрипт идемпотентный: его можно запускать сколько угодно раз, в том числе
# на сервере, развёрнутом прошлой версией — старые параметры подхватятся и
# будут переписаны на новые. Ничего не ломает при повторном запуске.
#
# Что делает:
#   1. sysctl: ip_forward, BBR + fq, буферы сокетов под высокую пропускную
#   2. собирает /usr/local/bin/forgefox-bridge (быстрый TUN-мост на C)
#   3. NAT/FORWARD правила для 10.0.0.0/8
#   4. sshd: PermitRootLogin/PermitTunnel, Compression no, AES-GCM первым
#
# Использование:
#   ./install.sh              — полная настройка
#   ./install.sh --check      — только показать текущее состояние, ничего не менять
#
set -uo pipefail

CHECK_ONLY=0
[ "${1:-}" = "--check" ] && CHECK_ONLY=1

BRIDGE_BIN=/usr/local/bin/forgefox-bridge
BRIDGE_SRC=/usr/local/src/forgefox-bridge.c
SYSCTL_CONF=/etc/sysctl.d/99-forgefox-vpn.conf
SSHD_CONFIG=/etc/ssh/sshd_config
SSHD_DROPIN_DIR=/etc/ssh/sshd_config.d
SSHD_DROPIN=$SSHD_DROPIN_DIR/00-forgefox-vpn.conf

RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; DIM=$'\033[2m'; RST=$'\033[0m'
ok()   { echo "  ${GRN}✓${RST} $*"; }
info() { echo "  ${DIM}·${RST} $*"; }
warn() { echo "  ${YLW}!${RST} $*"; }
err()  { echo "  ${RED}✗${RST} $*"; }

echo
echo "══════════════════════════════════════════════"
echo " ForgeFox VPN — настройка сервера"
echo "══════════════════════════════════════════════"

if [ "$(id -u)" -ne 0 ]; then
  err "Нужны права root. Запустите через sudo."
  exit 1
fi

# ── 0. Что уже настроено ──────────────────────────────────────────────────────
echo
echo "[0/5] Текущее состояние"

HAS_BRIDGE=0; [ -x "$BRIDGE_BIN" ] && HAS_BRIDGE=1
HAS_OLD=0
if [ -f "$SYSCTL_CONF" ] && ! grep -q "tcp_congestion_control" "$SYSCTL_CONF" 2>/dev/null; then
  HAS_OLD=1
fi

if [ "$HAS_BRIDGE" = 1 ]; then
  ok "forgefox-bridge установлен ($("$BRIDGE_BIN" 2>&1 | head -1 >/dev/null && echo ok || echo ok))"
else
  info "forgefox-bridge не установлен — сервер работает на медленном Python-мосту"
fi
[ "$HAS_OLD" = 1 ] && info "найден конфиг от старой версии — будет обновлён"
info "ядро: $(uname -r)"
info "текущий congestion control: $(sysctl -n net.ipv4.tcp_congestion_control 2>/dev/null || echo '?')"

if [ "$CHECK_ONLY" = 1 ]; then
  echo
  echo "Режим --check: изменения не вносились."
  exit 0
fi

# ── 1. sysctl ─────────────────────────────────────────────────────────────────
echo
echo "[1/5] Параметры ядра"

# BBR доступен с 4.9; если модуля нет — остаёмся на текущем алгоритме.
CC=cubic
if modprobe tcp_bbr 2>/dev/null || grep -q bbr /proc/sys/net/ipv4/tcp_available_congestion_control 2>/dev/null; then
  if grep -q bbr /proc/sys/net/ipv4/tcp_available_congestion_control 2>/dev/null; then
    CC=bbr
    echo tcp_bbr > /etc/modules-load.d/forgefox-bbr.conf
  fi
fi
[ "$CC" = bbr ] && ok "BBR доступен" || warn "BBR недоступен (ядро $(uname -r)) — остаёмся на cubic"

cat > "$SYSCTL_CONF" <<EOF
# ForgeFox VPN — генерируется install.sh, правки будут перезаписаны
net.ipv4.ip_forward=1

# Очередь + congestion control: BBR не схлопывает окно на потерях,
# что критично для TCP-в-TCP внутри SSH-туннеля.
net.core.default_qdisc=fq
net.ipv4.tcp_congestion_control=$CC

# Буферы под высокую пропускную (BDP на 500 Мбит × 100 мс ≈ 6 МБ)
net.core.rmem_max=67108864
net.core.wmem_max=67108864
net.ipv4.tcp_rmem=4096 87380 67108864
net.ipv4.tcp_wmem=4096 65536 67108864
net.core.netdev_max_backlog=16384
net.core.somaxconn=8192

net.ipv4.tcp_mtu_probing=1
net.ipv4.tcp_slow_start_after_idle=0
EOF

sysctl -p "$SYSCTL_CONF" >/dev/null 2>&1 && ok "применено: $SYSCTL_CONF" \
  || warn "часть параметров не применилась (см. sysctl -p $SYSCTL_CONF)"

# TUN-модуль
modprobe tun 2>/dev/null
echo tun > /etc/modules-load.d/tun.conf
ok "модуль tun загружен"

# ── 2. Сборка моста ───────────────────────────────────────────────────────────
echo
echo "[2/5] TUN-мост"

SELF_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
mkdir -p /usr/local/src

# Исходник берём из репозитория, если скрипт запущен рядом с ним,
# иначе — из встроенной копии ниже.
if [ -f "$SELF_DIR/server/forgefox-bridge.c" ]; then
  cp "$SELF_DIR/server/forgefox-bridge.c" "$BRIDGE_SRC"
  info "исходник взят из $SELF_DIR/server/"
else
  sed -n '/^# ---BRIDGE-SOURCE-BEGIN---$/,/^# ---BRIDGE-SOURCE-END---$/p' "${BASH_SOURCE[0]}" \
    | sed '1d;$d;s/^# \{0,1\}//' > "$BRIDGE_SRC"
  info "исходник взят из встроенной копии"
fi

if ! command -v gcc >/dev/null 2>&1; then
  info "устанавливаю gcc…"
  if   command -v apt-get >/dev/null 2>&1; then apt-get update -qq >/dev/null 2>&1; DEBIAN_FRONTEND=noninteractive apt-get install -y -qq gcc >/dev/null 2>&1
  elif command -v dnf     >/dev/null 2>&1; then dnf install -y -q gcc >/dev/null 2>&1
  elif command -v yum     >/dev/null 2>&1; then yum install -y -q gcc >/dev/null 2>&1
  elif command -v apk     >/dev/null 2>&1; then apk add --quiet gcc musl-dev >/dev/null 2>&1
  fi
fi

if command -v gcc >/dev/null 2>&1; then
  if gcc -O2 -o "$BRIDGE_BIN.new" "$BRIDGE_SRC" -lpthread 2>/tmp/forgefox-cc.log; then
    mv -f "$BRIDGE_BIN.new" "$BRIDGE_BIN"
    chmod 755 "$BRIDGE_BIN"
    ok "собран $BRIDGE_BIN"
  else
    rm -f "$BRIDGE_BIN.new"
    err "сборка не удалась:"
    sed 's/^/      /' /tmp/forgefox-cc.log | head -20
    warn "клиент автоматически откатится на Python-мост (медленный, но рабочий)"
  fi
else
  warn "gcc недоступен — мост не собран, клиент откатится на Python-мост"
fi

# ── 3. NAT ────────────────────────────────────────────────────────────────────
echo
echo "[3/5] NAT"

PRIMARY_IF=$(ip route show default | awk '{for(i=1;i<NF;i++) if($i=="dev") print $(i+1); exit}')
if [ -z "$PRIMARY_IF" ]; then
  err "не найден интерфейс с маршрутом по умолчанию — NAT не настроен"
else
  info "внешний интерфейс: $PRIMARY_IF"
  # Чистим правило от старой версии скрипта (оно было с /24)
  iptables -t nat -D POSTROUTING -s 10.0.0.0/24 -o "$PRIMARY_IF" -j MASQUERADE 2>/dev/null
  # Дедупликация: снимаем все копии, ставим одну
  while iptables -t nat -C POSTROUTING -s 10.0.0.0/8 -o "$PRIMARY_IF" -j MASQUERADE 2>/dev/null; do
    iptables -t nat -D POSTROUTING -s 10.0.0.0/8 -o "$PRIMARY_IF" -j MASQUERADE
  done
  iptables -t nat -A POSTROUTING -s 10.0.0.0/8 -o "$PRIMARY_IF" -j MASQUERADE
  ok "MASQUERADE 10.0.0.0/8 → $PRIMARY_IF"

  iptables -C FORWARD -s 10.0.0.0/8 -j ACCEPT 2>/dev/null || iptables -I FORWARD 1 -s 10.0.0.0/8 -j ACCEPT
  iptables -C FORWARD -d 10.0.0.0/8 -j ACCEPT 2>/dev/null || iptables -I FORWARD 1 -d 10.0.0.0/8 -j ACCEPT
  ok "FORWARD разрешён для 10.0.0.0/8"

  # Клампинг MSS. Без него удалённый сайт узнаёт MSS только из SYN клиента, и
  # любой сбой определения MTU (или MTU туннеля, выставленный клиентом иначе)
  # даёт пакеты крупнее туннельного MTU. Такой пакет молча дропается при записи
  # в TUN: SYN проходит (он мелкий), а TLS-handshake встаёт — сайт «висит» и
  # отваливается по таймауту, тогда как мелкий трафик работает.
  # --clamp-mss-to-pmtu берёт MTU исходящего маршрута, то есть для направления
  # «в туннель» это MTU TUN-интерфейса, а не 1500 физического.
  if iptables -t mangle -C FORWARD -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; then
    ok "MSS уже клампится"
  elif iptables -t mangle -I FORWARD 1 -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; then
    ok "MSS клампится по PMTU (иначе TLS-handshake вставал бы на крупных пакетах)"
  else
    warn "не удалось поставить правило TCPMSS — нет модуля xt_TCPMSS?"
  fi

  if command -v netfilter-persistent >/dev/null 2>&1; then
    netfilter-persistent save >/dev/null 2>&1 && ok "правила сохранены (netfilter-persistent)"
  elif command -v iptables-save >/dev/null 2>&1 && [ -d /etc/iptables ]; then
    iptables-save > /etc/iptables/rules.v4 && ok "правила сохранены (/etc/iptables/rules.v4)"
  else
    warn "правила не сохранены на диск — после перезагрузки перезапустите install.sh"
  fi
fi

# ── 4. sshd ───────────────────────────────────────────────────────────────────
echo
echo "[4/5] SSH-демон"

# sshd почти всегда в /usr/sbin, которого может не быть в PATH под sudo.
SSHD_BIN=$(command -v sshd 2>/dev/null || true)
for c in /usr/sbin/sshd /usr/local/sbin/sshd /sbin/sshd; do
  [ -n "$SSHD_BIN" ] && break
  [ -x "$c" ] && SSHD_BIN=$c
done

if [ -z "$SSHD_BIN" ] || [ ! -f "$SSHD_CONFIG" ]; then
  # Нет смысла сочинять конфиг там, где SSH-сервер не установлен: рабочий
  # сервер так выглядеть не может, а мы бы создали файл, который потом
  # некому проверить и нечем откатить.
  warn "SSH-сервер не найден (нет ${SSHD_BIN:-бинарника sshd}${SSHD_BIN:+, нет $SSHD_CONFIG}) — раздел пропущен"
  info "мост и параметры ядра настроены; sshd донастройте на машине, где он есть"
  SSHD_SKIPPED=1
else
  SSHD_SKIPPED=0
fi

if [ "$SSHD_SKIPPED" = 0 ]; then

SSHD_BACKUP=/etc/ssh/sshd_config.forgefox-backup
[ -f "$SSHD_BACKUP" ] || cp "$SSHD_CONFIG" "$SSHD_BACKUP"

# Нужные нам директивы. Compression no — сжимать уже зашифрованный
# туннельный трафик бессмысленно, это только жжёт CPU.
# AES-GCM первым — на x86 с AES-NI он заметно быстрее chacha20.
read -r -d '' FORGEFOX_SSHD <<'EOF'
# ForgeFox VPN — генерируется install.sh
PermitRootLogin yes
PermitTunnel yes
Compression no
Ciphers aes256-gcm@openssh.com,aes128-gcm@openssh.com,chacha20-poly1305@openssh.com,aes256-ctr
ClientAliveInterval 30
ClientAliveCountMax 4
UseDNS no
EOF

# В OpenSSH выигрывает ПЕРВОЕ вхождение директивы, а в новых дистрибутивах
# Include стоит в начале sshd_config. Поэтому на таких системах пишем свой
# drop-in (имя с "00-" сортируется раньше прочих), а конфликтующие строки
# из основного файла убираем — иначе они могут перебить наш конфиг.
DIRECTIVES="PermitRootLogin PermitTunnel Compression Ciphers ClientAliveInterval ClientAliveCountMax UseDNS"

strip_directives() {  # $1 = файл
  local f=$1 d
  [ -f "$f" ] || return 0
  for d in $DIRECTIVES; do
    sed -i -E "s/^([[:space:]]*${d}[[:space:]]+)/#forgefox-disabled \1/I" "$f"
  done
}

if grep -qiE '^[[:space:]]*Include[[:space:]]+/etc/ssh/sshd_config\.d/' "$SSHD_CONFIG" 2>/dev/null; then
  mkdir -p "$SSHD_DROPIN_DIR"
  printf '%s\n' "$FORGEFOX_SSHD" > "$SSHD_DROPIN"
  chmod 644 "$SSHD_DROPIN"
  strip_directives "$SSHD_CONFIG"
  # и в чужих drop-in'ах, которые сортируются после нашего
  for f in "$SSHD_DROPIN_DIR"/*.conf; do
    [ "$f" = "$SSHD_DROPIN" ] && continue
    [ -f "$f" ] && strip_directives "$f"
  done
  ok "конфиг записан в $SSHD_DROPIN"
else
  strip_directives "$SSHD_CONFIG"
  { echo; printf '%s\n' "$FORGEFOX_SSHD"; } >> "$SSHD_CONFIG"
  ok "конфиг дописан в $SSHD_CONFIG"
fi

# Проверяем перед перезапуском — сломанный sshd на удалённом сервере
# означает потерю доступа, поэтому при ошибке откатываемся.
if "$SSHD_BIN" -t 2>/tmp/forgefox-sshd.log; then
  ok "конфигурация валидна ($SSHD_BIN -t)"
  if   systemctl restart sshd  2>/dev/null; then ok "sshd перезапущен"
  elif systemctl restart ssh   2>/dev/null; then ok "ssh перезапущен"
  elif service ssh restart     2>/dev/null; then ok "ssh перезапущен"
  else warn "не удалось перезапустить SSH — сделайте это вручную"
  fi
  systemctl restart ssh.socket 2>/dev/null
else
  err "sshd -t ругается на конфиг:"
  sed 's/^/      /' /tmp/forgefox-sshd.log | head -10
  cp "$SSHD_BACKUP" "$SSHD_CONFIG"
  rm -f "$SSHD_DROPIN"
  err "изменения откачены, SSH НЕ перезапущен — доступ сохранён"
  exit 1
fi

fi  # SSHD_SKIPPED

# ── 5. Итог ───────────────────────────────────────────────────────────────────
echo
echo "[5/5] Проверка"

[ -x "$BRIDGE_BIN" ] && ok "мост:      $BRIDGE_BIN (быстрый путь)" \
                     || warn "мост:      не собран → Python-фоллбэк (~15-25 Мбит)"
ok "forward:   $(sysctl -n net.ipv4.ip_forward)"
ok "cc/qdisc:  $(sysctl -n net.ipv4.tcp_congestion_control) / $(sysctl -n net.core.default_qdisc)"
[ "$SSHD_SKIPPED" = 0 ] && ok "бэкап sshd: $SSHD_BACKUP" \
                        || warn "sshd:      пропущен (SSH-сервер не найден)"

echo
echo "══════════════════════════════════════════════"
echo " Готово. Сервер принимает SSH VPN-подключения."
echo "══════════════════════════════════════════════"
echo
echo "Диагностика во время нагрузки:"
echo "  top -b -n3 -d1 | grep -E 'forgefox|python3'"
echo "  (forgefox-bridge должен есть заметно меньше одного ядра)"
echo

exit 0

# ---BRIDGE-SOURCE-BEGIN---
# /*
#  * forgefox-bridge — двухпоточный TUN-мост для ForgeFoxVPN.
#  *
#  * Заменяет однопоточный Python-скрипт, который клиент гонял через
#  * exec по SSH-каналу: обвязка TUN + NAT осталась та же, но данные
#  * пересылаются двумя потоками (входящий/исходящий) с батчингом
#  * пакетов в один write(), а не по одному с ~5 syscall'ами на пакет.
#  *
#  * Протокол кадра (совместим с клиентом): [2 байта длины BE][пакет].
#  *
#  * Сборка: gcc -O2 -o forgefox-bridge forgefox-bridge.c -lpthread
#  * Запуск: forgefox-bridge <ip-сервера-в-туннеле> <mtu>
#  */
# #define _GNU_SOURCE
# #include <stdio.h>
# #include <stdlib.h>
# #include <string.h>
# #include <unistd.h>
# #include <fcntl.h>
# #include <errno.h>
# #include <poll.h>
# #include <pthread.h>
# #include <sys/ioctl.h>
# #include <sys/socket.h>
# #include <linux/if.h>
# #include <linux/if_tun.h>
#
# #define MAXPKT 65535
# #define BATCH  (1024 * 1024)   /* до ~1 МБ пакетов за один write на stdout */
#
# static int tunfd;
#
# static int xwrite(int fd, const unsigned char *b, size_t n) {
#     size_t p = 0;
#     while (p < n) {
#         ssize_t w = write(fd, b + p, n - p);
#         if (w > 0) { p += (size_t)w; continue; }
#         if (w < 0 && errno == EINTR) continue;
#         return -1;
#     }
#     return 0;
# }
#
# /* Читает ровно n байт: 0 — успех, -1 — EOF/ошибка.
#    Проверка p > n формально исключает underflow в n - p: read() не вправе
#    вернуть больше запрошенного, но без этой проверки того не видно ни
#    компилятору, ни читателю. */
# static int xread(int fd, unsigned char *b, size_t n) {
#     size_t p = 0;
#     while (p < n) {
#         ssize_t r = read(fd, b + p, n - p);
#         if (r > 0) {
#             p += (size_t)r;
#             if (p > n) return -1;
#             continue;
#         }
#         if (r < 0 && errno == EINTR) continue;
#         return -1;
#     }
#     return 0;
# }
#
# /* Обвязка сети: сам вызов может не пройти, но упасть из-за этого нельзя —
#    мост обязан продолжать гнать трафик. */
# static void sh(const char *c) {
#     if (system(c) == -1) perror("system");
# }
#
# /* stdin -> tun: разбор кадров клиента, запись в TUN */
# static void *down_thread(void *_) {
#     unsigned char hdr[2], pkt[MAXPKT];
#     struct pollfd po = { .fd = tunfd, .events = POLLOUT };
#     (void)_;
#     for (;;) {
#         if (xread(0, hdr, 2)) _exit(0);   /* EOF / ошибка канала */
#         unsigned len = ((unsigned)hdr[0] << 8) | hdr[1];
#         if (!len || len > MAXPKT) _exit(0);
#         if (xread(0, pkt, len)) _exit(0);
#         for (;;) {                  /* ровно один пакет за один write() в TUN */
#             ssize_t w = write(tunfd, pkt, len);
#             if (w >= 0) break;
#             if (errno == EINTR) continue;
#             if (errno == EAGAIN) { poll(&po, 1, 10); continue; }
#             break;                  /* очередь забита — дропаем, как роутер */
#         }
#     }
# }
#
# /* tun -> stdout: батчинг пакетов в один write, EAGAIN = очередь пуста */
# static void *up_thread(void *_) {
#     unsigned char *buf = malloc(BATCH);
#     struct pollfd po = { .fd = tunfd, .events = POLLIN };
#     (void)_;
#     if (!buf) _exit(1);
#     for (;;) {
#         size_t used = 0;
#         while (used + 2 + MAXPKT <= BATCH) {
#             ssize_t r = read(tunfd, buf + used + 2, MAXPKT);
#             if (r > 0) {
#                 buf[used]     = (unsigned char)((r >> 8) & 0xff);
#                 buf[used + 1] = (unsigned char)(r & 0xff);
#                 used += 2 + (size_t)r;
#                 continue;
#             }
#             if (r < 0 && errno == EINTR) continue;
#             break;                  /* EAGAIN — очередь опустела */
#         }
#         if (used) { if (xwrite(1, buf, used)) _exit(0); continue; }
#         if (poll(&po, 1, -1) < 0 && errno != EINTR) _exit(0);
#     }
# }
#
# int main(int argc, char **argv) {
#     if (argc < 3) { fprintf(stderr, "usage: %s <ip> <mtu>\n", argv[0]); return 1; }
#     const char *ip = argv[1], *mtu = argv[2];
#     char cmd[512], dev[IFNAMSIZ] = {0}, net[64] = {0};
#     struct ifreq ifr;
#
#     if ((tunfd = open("/dev/net/tun", O_RDWR)) < 0) { perror("tun"); return 1; }
#     memset(&ifr, 0, sizeof ifr);
#     ifr.ifr_flags = IFF_TUN | IFF_NO_PI;
#     if (ioctl(tunfd, TUNSETIFF, &ifr) < 0) { perror("TUNSETIFF"); return 1; }
#     snprintf(dev, sizeof dev, "%s", ifr.ifr_name);
#     fcntl(tunfd, F_SETFL, fcntl(tunfd, F_GETFL) | O_NONBLOCK);
#
#     fcntl(0, F_SETPIPE_SZ, 1 << 20);   /* толще pipe к sshd */
#     fcntl(1, F_SETPIPE_SZ, 1 << 20);
#
#     snprintf(cmd, sizeof cmd,
#         "ip link set %s up && ip addr replace %s/24 dev %s && "
#         "ip link set dev %s mtu %s txqueuelen 10000", dev, ip, dev, dev, mtu);
#     sh(cmd);
#
#     { int a, b, c, d; if (sscanf(ip, "%d.%d.%d.%d", &a, &b, &c, &d) == 4)
#         snprintf(net, sizeof net, "%d.%d.%d.0/24", a, b, c); }
#
#     sh("sysctl -w net.ipv4.ip_forward=1 >/dev/null 2>&1");
#     /* NAT ставим только если подсеть разобралась — иначе правило уйдёт с мусором */
#     { FILE *f = *net ? popen("ip route show default | awk '{for(i=1;i<NF;i++) if($i==\"dev\") print $(i+1)}'", "r") : NULL;
#       char out[64] = {0};
#       if (!*net) fprintf(stderr, "cannot parse tunnel ip '%s', NAT not configured\n", ip);
#       if (f && fgets(out, sizeof out, f)) {
#         out[strcspn(out, "\n")] = 0;
#         if (*out) {
# /*        Сначала пробуем общее правило на 10.0.0.0/8, которое ставит install.sh.
#              Если оно есть — своё, на /24, не добавляем. Мост не может убрать за
#              собой правило при выходе (его убивает закрытие SSH-канала, SIGKILL
#              обработчику не достаётся), поэтому каждая сессия оставляла бы в
#              nat-таблице ещё одну строку — на живых серверах их накопились десятки. */
#           snprintf(cmd, sizeof cmd,
#                    "iptables -t nat -C POSTROUTING -s 10.0.0.0/8 -o %s -j MASQUERADE 2>/dev/null || "
#                    "iptables -t nat -C POSTROUTING -s %s -o %s -j MASQUERADE 2>/dev/null || "
#                    "iptables -t nat -A POSTROUTING -s %s -o %s -j MASQUERADE", out, net, out, net, out);
#           sh(cmd);
#           snprintf(cmd, sizeof cmd,
#                    "iptables -C FORWARD -s 10.0.0.0/8 -j ACCEPT 2>/dev/null || "
#                    "iptables -C FORWARD -i %s -j ACCEPT 2>/dev/null || iptables -I FORWARD 1 -i %s -j ACCEPT", dev, dev);
#           sh(cmd);
#           snprintf(cmd, sizeof cmd,
#                    "iptables -C FORWARD -d 10.0.0.0/8 -j ACCEPT 2>/dev/null || "
#                    "iptables -C FORWARD -o %s -j ACCEPT 2>/dev/null || iptables -I FORWARD 1 -o %s -j ACCEPT", dev, dev);
#           sh(cmd);
#           fprintf(stderr, "nat ready via %s on %s\n", out, dev);
#         }
#       }
#       if (f) pclose(f);
#     }
#
#     pthread_t t;
#     if (pthread_create(&t, NULL, down_thread, NULL) != 0) {
#         perror("pthread_create");   /* иначе туннель молча стал бы односторонним */
#         return 1;
#     }
#     up_thread(NULL);
#     return 0;
# }
# ---BRIDGE-SOURCE-END---
