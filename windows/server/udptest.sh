#!/usr/bin/env bash
# UDP-егресс и лимиты conntrack. ТОЛЬКО ЧТЕНИЕ.
set -uo pipefail

echo "── DNS по UDP/53 наружу (именно это идёт через туннель в Proxy-режиме) ──"
for r in 1.1.1.1 8.8.8.8 9.9.9.9 77.88.8.8; do
  if command -v dig >/dev/null 2>&1; then
    a=$(dig +time=3 +tries=1 +short @"$r" google.com A 2>/dev/null | head -1)
  else
    a=$(timeout 5 nslookup google.com "$r" 2>/dev/null | awk '/^Address: /{print $2; exit}')
  fi
  [ -n "$a" ] && echo "  ✓ UDP/53 → $r  (google.com = $a)" || echo "  ✗ UDP/53 → $r  МОЛЧИТ"
done

echo
echo "── DNS по TCP/53 (для сравнения) ──"
for r in 1.1.1.1 8.8.8.8; do
  timeout 5 bash -c "exec 3<>/dev/tcp/$r/53" 2>/dev/null && echo "  ✓ TCP/53 → $r" || echo "  ✗ TCP/53 → $r"
done

echo
echo "── Прочий UDP: QUIC/443 и NTP/123 ──"
timeout 4 bash -c 'exec 3<>/dev/udp/216.239.35.0/123 && printf "\x1b%*s" 47 "" >&3 && timeout 3 head -c1 <&3 >/dev/null' 2>/dev/null \
  && echo "  ✓ NTP/123 отвечает" || echo "  ! NTP/123 без ответа (не всегда показательно)"

echo
echo "── conntrack ──"
echo "  max=$(sysctl -n net.netfilter.nf_conntrack_max 2>/dev/null || echo '?')  count=$(cat /proc/sys/net/netfilter/nf_conntrack_count 2>/dev/null || echo '?')"
echo "  udp_timeout=$(sysctl -n net.netfilter.nf_conntrack_udp_timeout 2>/dev/null || echo '?')"

echo
echo "── Дропы на интерфейсах ──"
ip -s link show eth0 2>/dev/null | tail -4 | sed 's/^/  /'
for d in $(ls /sys/class/net | grep -E '^tun'); do
  echo "  $d: rx=$(cat /sys/class/net/$d/statistics/rx_packets) tx=$(cat /sys/class/net/$d/statistics/tx_packets) rxdrop=$(cat /sys/class/net/$d/statistics/rx_dropped) txdrop=$(cat /sys/class/net/$d/statistics/tx_dropped)"
done

echo
echo "── systemd-resolved: куда он ходит ──"
resolvectl status 2>/dev/null | grep -iE "current dns|dns servers|fallback" | head -5 | sed 's/^/  /'
