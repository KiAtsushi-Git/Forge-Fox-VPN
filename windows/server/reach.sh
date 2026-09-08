#!/usr/bin/env bash
# Матрица связности: куда сервер может достучаться сам. ТОЛЬКО ЧТЕНИЕ.
set -uo pipefail

SITES="google.com www.youtube.com cloudflare.com 2ip.ru yandex.ru vk.com github.com
       discord.com telegram.org web.whatsapp.com netflix.com speedtest.net
       api.ipify.org wikipedia.org reddit.com x.com instagram.com"

printf "%-22s %-16s %s\n" "ХОСТ" "IP" "TCP:443"
echo "──────────────────────────────────────────────────────"
FAIL=0; TOT=0
for s in $SITES; do
  ip=$(getent ahostsv4 "$s" 2>/dev/null | awk 'NR==1{print $1}')
  if [ -z "$ip" ]; then
    printf "%-22s %-16s %s\n" "$s" "-" "✗ DNS не резолвится"
    FAIL=$((FAIL+1)); TOT=$((TOT+1)); continue
  fi
  TOT=$((TOT+1))
  if timeout 6 bash -c "exec 3<>/dev/tcp/$ip/443" 2>/dev/null; then
    printf "%-22s %-16s %s\n" "$s" "$ip" "✓"
  else
    printf "%-22s %-16s %s\n" "$s" "$ip" "✗ ТАЙМАУТ"
    FAIL=$((FAIL+1))
  fi
done
echo "──────────────────────────────────────────────────────"
echo "недоступно: $FAIL из $TOT"
