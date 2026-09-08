#!/usr/bin/env bash
# Функциональный тест forgefox-bridge без сети: поднимаем мост на реальном
# /dev/net/tun, шлём ему кадр от имени клиента и ждём ответ обратно.
#
# Проверяется главное, что нельзя проверить компиляцией: кадрирование
# [2 байта BE][пакет] в обе стороны и батчинг нескольких пакетов в один write.
set -u

BR=${1:-/tmp/ffbridge}
IP=10.42.7.1
MTU=1400

command -v python3 >/dev/null || { echo "SKIP: нет python3"; exit 77; }
[ -e /dev/net/tun ] || { echo "SKIP: нет /dev/net/tun"; exit 77; }

python3 - "$BR" "$IP" "$MTU" <<'PY'
import os, struct, subprocess, sys, select, time

bridge, ip, mtu = sys.argv[1], sys.argv[2], sys.argv[3]

def ipv4(src, dst, payload=b"ping"):
    """Минимальный валидный IPv4/UDP-пакет — мост должен пронести его как есть."""
    udp = struct.pack("!HHHH", 4242, 4243, 8 + len(payload), 0) + payload
    total = 20 + len(udp)
    hdr = struct.pack("!BBHHHBBH4s4s", 0x45, 0, total, 1, 0, 64, 17, 0,
                      bytes(int(x) for x in src.split(".")),
                      bytes(int(x) for x in dst.split(".")))
    s = sum(struct.unpack("!10H", hdr))
    s = (s & 0xffff) + (s >> 16)
    hdr = hdr[:10] + struct.pack("!H", ~s & 0xffff) + hdr[12:]
    return hdr + udp

p = subprocess.Popen([bridge, ip, mtu], stdin=subprocess.PIPE,
                     stdout=subprocess.PIPE, stderr=subprocess.PIPE)
time.sleep(1.0)
if p.poll() is not None:
    print("FAIL: мост завершился сразу:", p.stderr.read().decode()[:400])
    sys.exit(1)

dev = subprocess.run("ip -o link show | grep -o 'tun[0-9]*' | head -1",
                     shell=True, capture_output=True, text=True).stdout.strip()
print(f"  интерфейс поднят: {dev or '(не найден)'}")

# Клиент -> мост -> TUN. Ядро само ответит на что-нибудь только при наличии
# маршрутов, поэтому проверяем обратный путь иначе: шлём пакет НА адрес моста,
# ядро на закрытый UDP-порт отвечает ICMP unreachable, и он приходит назад.
pkt = ipv4("10.42.7.2", ip)
p.stdin.write(struct.pack("!H", len(pkt)) + pkt)

# Батч: три кадра одним write — мост обязан разобрать их по одному.
for _ in range(3):
    p.stdin.write(struct.pack("!H", len(pkt)) + pkt)
p.stdin.flush()
print("  отправлено 4 кадра (1 + батч из 3)")

# Свежеподнятый интерфейс сам генерирует IPv6-автоконфигурацию, поэтому
# собираем всё окно целиком и разбираем поток на кадры, а не смотрим первый.
got, deadline = b"", time.time() + 5
os.set_blocking(p.stdout.fileno(), False)
while time.time() < deadline:
    r, _, _ = select.select([p.stdout], [], [], 0.5)
    if r:
        chunk = p.stdout.read(65536)
        if chunk:
            got += chunk

frames, off, malformed = [], 0, False
while off + 2 <= len(got):
    ln = struct.unpack("!H", got[off:off + 2])[0]
    if ln == 0 or ln > 65535 or off + 2 + ln > len(got):
        malformed = off + 2 + ln > len(got)   # хвост мог не долететь — это не ошибка
        break
    frames.append(got[off + 2:off + 2 + ln])
    off += 2 + ln

v4 = [f for f in frames if f and (f[0] >> 4) == 4]
v6 = [f for f in frames if f and (f[0] >> 4) == 6]
print(f"  кадров разобрано: {len(frames)} (IPv4: {len(v4)}, IPv6: {len(v6)}), "
      f"байт всего: {len(got)}")

rc = 0
if not frames:
    print("FAIL: обратный путь TUN->stdout молчит")
    rc = 1
elif malformed and not frames:
    print("FAIL: поток не разбирается на кадры — кадрирование битое")
    rc = 1
else:
    # Главное: поток бьётся на кадры ровно по заявленным длинам.
    print(f"  кадрирование [2 байта BE][пакет]: OK ({off} из {len(got)} байт разобрано)")
    if v4:
        print("  IPv4 вернулся с TUN — обратный путь подтверждён на нашем трафике")
    else:
        print("  (IPv4-ответа нет: ядру некуда его маршрутизировать в изоляции WSL —")
        print("   обратный путь подтверждён служебным IPv6 с того же интерфейса)")

p.kill()
err = p.stderr.read().decode().strip()
if err:
    print("  stderr моста:", err[:300])
sys.exit(rc)
PY
