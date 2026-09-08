#!/usr/bin/env bash
# Характер блокировки: точечная по IP или по всей подсети. ТОЛЬКО ЧТЕНИЕ.
set -uo pipefail

t() {  # t <ip> <port>
  if timeout 6 bash -c "exec 3<>/dev/tcp/$1/$2" 2>/dev/null; then
    printf "  ✓ %-18s :%s\n" "$1" "$2"
  else
    printf "  ✗ %-18s :%s  таймаут\n" "$1" "$2"
  fi
}

echo "── Google: разные IP из разных подсетей ──"
for i in 142.251.39.238 142.251.142.110 172.217.21.238 142.250.185.78 216.58.212.142; do t "$i" 443; done

echo
echo "── Google по 80 порту ──"
for i in 142.251.39.238 142.250.185.78; do t "$i" 80; done

echo
echo "── 2ip.ru: сам хост и соседи по /24 (Hetzner) ──"
for i in 188.40.167.82 188.40.167.83 188.40.167.1; do t "$i" 443; done
t 188.40.167.82 80

echo
echo "── Hetzner вообще (другая подсеть) ──"
for i in 88.198.0.1 5.9.0.1; do t "$i" 443; done

echo
echo "── Reddit ──"
for i in 151.101.65.140 151.101.1.140; do t "$i" 443; done

echo
echo "── Контроль: заведомо рабочие ──"
for i in 104.16.132.229 140.82.121.4 1.1.1.1; do t "$i" 443; done
