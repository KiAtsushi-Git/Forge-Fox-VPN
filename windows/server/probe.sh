#!/usr/bin/env bash
# Точечная проверка. ТОЛЬКО ЧТЕНИЕ.
set -uo pipefail

echo "── PATH, который видит клиент (неинтерактивный ssh exec) ──"
echo "  PATH=$PATH"
if command -v forgefox-bridge >/dev/null 2>&1; then
  echo "  command -v forgefox-bridge → $(command -v forgefox-bridge)  ✓ пойдёт БЫСТРЫЙ мост"
else
  echo "  command -v forgefox-bridge → НЕ НАЙДЕН  ✗ клиент откатится на Python"
fi
ls -la /usr/local/bin/forgefox-bridge 2>&1 | sed 's/^/  /'

echo
echo "── MTU на туннельных интерфейсах ──"
ip -o link show 2>/dev/null | grep -E 'tun|tap' | sed 's/^/  /'
echo "  eth0: $(ip -o link show eth0 2>/dev/null | grep -o 'mtu [0-9]*')"

echo
echo "── Процессы мостов: когда стартовали ──"
ps -eo pid,lstart,etime,comm 2>/dev/null | grep -E 'forgefox|python3' | sed 's/^/  /' || echo "  нет"

echo
echo "── Живые SSH-сессии (кроме моей) ──"
ss -tnp 2>/dev/null | grep ':22 ' | sed 's/^/  /' | head -10

echo
echo "── DNS сервера ──"
cat /etc/resolv.conf 2>/dev/null | grep -v '^#' | grep -v '^$' | sed 's/^/  /'
getent hosts 2ip.ru 2>&1 | sed 's/^/  2ip.ru → /' || echo "  2ip.ru НЕ РЕЗОЛВИТСЯ"

echo
echo "── Прямой TCP до 2ip.ru (188.40.167.82:443), в обход DNS ──"
timeout 8 bash -c 'exec 3<>/dev/tcp/188.40.167.82/443' 2>/dev/null \
  && echo "  ✓ TCP 443 открывается" || echo "  ✗ TCP 443 НЕ открывается — сервер не пускают к этому хосту"
echo "  curl по IP: $(curl -s -o /dev/null -w '%{http_code}' --max-time 8 --resolve 2ip.ru:443:188.40.167.82 https://2ip.ru 2>&1)"

echo
echo "── Пара популярных целей ──"
for t in 1.1.1.1:443 8.8.8.8:443 142.250.185.78:443; do
  h=${t%:*}; p=${t#*:}
  timeout 5 bash -c "exec 3<>/dev/tcp/$h/$p" 2>/dev/null \
    && echo "  ✓ $t" || echo "  ✗ $t"
done
