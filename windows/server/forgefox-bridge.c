/*
 * forgefox-bridge — двухпоточный TUN-мост для ForgeFoxVPN.
 *
 * Заменяет однопоточный Python-скрипт, который клиент гонял через
 * exec по SSH-каналу: обвязка TUN + NAT осталась та же, но данные
 * пересылаются двумя потоками (входящий/исходящий) с батчингом
 * пакетов в один write(), а не по одному с ~5 syscall'ами на пакет.
 *
 * Протокол кадра (совместим с клиентом): [2 байта длины BE][пакет].
 *
 * Сборка: gcc -O2 -o forgefox-bridge forgefox-bridge.c -lpthread
 * Запуск: forgefox-bridge <ip-сервера-в-туннеле> <mtu>
 */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <poll.h>
#include <pthread.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <linux/if.h>
#include <linux/if_tun.h>

#define MAXPKT 65535
#define BATCH  (1024 * 1024)   /* до ~1 МБ пакетов за один write на stdout */

static int tunfd;

static int xwrite(int fd, const unsigned char *b, size_t n) {
    size_t p = 0;
    while (p < n) {
        ssize_t w = write(fd, b + p, n - p);
        if (w > 0) { p += (size_t)w; continue; }
        if (w < 0 && errno == EINTR) continue;
        return -1;
    }
    return 0;
}

/* Читает ровно n байт: 0 — успех, -1 — EOF/ошибка.
   Проверка p > n формально исключает underflow в n - p: read() не вправе
   вернуть больше запрошенного, но без этой проверки того не видно ни
   компилятору, ни читателю. */
static int xread(int fd, unsigned char *b, size_t n) {
    size_t p = 0;
    while (p < n) {
        ssize_t r = read(fd, b + p, n - p);
        if (r > 0) {
            p += (size_t)r;
            if (p > n) return -1;
            continue;
        }
        if (r < 0 && errno == EINTR) continue;
        return -1;
    }
    return 0;
}

/* Обвязка сети: сам вызов может не пройти, но упасть из-за этого нельзя —
   мост обязан продолжать гнать трафик. */
static void sh(const char *c) {
    if (system(c) == -1) perror("system");
}

/* stdin -> tun: разбор кадров клиента, запись в TUN */
static void *down_thread(void *_) {
    unsigned char hdr[2], pkt[MAXPKT];
    struct pollfd po = { .fd = tunfd, .events = POLLOUT };
    (void)_;
    for (;;) {
        if (xread(0, hdr, 2)) _exit(0);   /* EOF / ошибка канала */
        unsigned len = ((unsigned)hdr[0] << 8) | hdr[1];
        if (!len || len > MAXPKT) _exit(0);
        if (xread(0, pkt, len)) _exit(0);
        for (;;) {                  /* ровно один пакет за один write() в TUN */
            ssize_t w = write(tunfd, pkt, len);
            if (w >= 0) break;
            if (errno == EINTR) continue;
            if (errno == EAGAIN) { poll(&po, 1, 10); continue; }
            break;                  /* очередь забита — дропаем, как роутер */
        }
    }
}

/* tun -> stdout: батчинг пакетов в один write, EAGAIN = очередь пуста */
static void *up_thread(void *_) {
    unsigned char *buf = malloc(BATCH);
    struct pollfd po = { .fd = tunfd, .events = POLLIN };
    (void)_;
    if (!buf) _exit(1);
    for (;;) {
        size_t used = 0;
        while (used + 2 + MAXPKT <= BATCH) {
            ssize_t r = read(tunfd, buf + used + 2, MAXPKT);
            if (r > 0) {
                buf[used]     = (unsigned char)((r >> 8) & 0xff);
                buf[used + 1] = (unsigned char)(r & 0xff);
                used += 2 + (size_t)r;
                continue;
            }
            if (r < 0 && errno == EINTR) continue;
            break;                  /* EAGAIN — очередь опустела */
        }
        if (used) { if (xwrite(1, buf, used)) _exit(0); continue; }
        if (poll(&po, 1, -1) < 0 && errno != EINTR) _exit(0);
    }
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: %s <ip> <mtu>\n", argv[0]); return 1; }
    const char *ip = argv[1], *mtu = argv[2];
    char cmd[512], dev[IFNAMSIZ] = {0}, net[64] = {0};
    struct ifreq ifr;

    if ((tunfd = open("/dev/net/tun", O_RDWR)) < 0) { perror("tun"); return 1; }
    memset(&ifr, 0, sizeof ifr);
    ifr.ifr_flags = IFF_TUN | IFF_NO_PI;
    if (ioctl(tunfd, TUNSETIFF, &ifr) < 0) { perror("TUNSETIFF"); return 1; }
    snprintf(dev, sizeof dev, "%s", ifr.ifr_name);
    fcntl(tunfd, F_SETFL, fcntl(tunfd, F_GETFL) | O_NONBLOCK);

    fcntl(0, F_SETPIPE_SZ, 1 << 20);   /* толще pipe к sshd */
    fcntl(1, F_SETPIPE_SZ, 1 << 20);

    snprintf(cmd, sizeof cmd,
        "ip link set %s up && ip addr replace %s/24 dev %s && "
        "ip link set dev %s mtu %s txqueuelen 10000", dev, ip, dev, dev, mtu);
    sh(cmd);

    { int a, b, c, d; if (sscanf(ip, "%d.%d.%d.%d", &a, &b, &c, &d) == 4)
        snprintf(net, sizeof net, "%d.%d.%d.0/24", a, b, c); }

    sh("sysctl -w net.ipv4.ip_forward=1 >/dev/null 2>&1");
    /* NAT ставим только если подсеть разобралась — иначе правило уйдёт с мусором */
    { FILE *f = *net ? popen("ip route show default | awk '{for(i=1;i<NF;i++) if($i==\"dev\") print $(i+1)}'", "r") : NULL;
      char out[64] = {0};
      if (!*net) fprintf(stderr, "cannot parse tunnel ip '%s', NAT not configured\n", ip);
      if (f && fgets(out, sizeof out, f)) {
        out[strcspn(out, "\n")] = 0;
        if (*out) {
          /* Сначала пробуем общее правило на 10.0.0.0/8, которое ставит install.sh.
             Если оно есть — своё, на /24, не добавляем. Мост не может убрать за
             собой правило при выходе (его убивает закрытие SSH-канала, SIGKILL
             обработчику не достаётся), поэтому каждая сессия оставляла бы в
             nat-таблице ещё одну строку — на живых серверах их накопились десятки. */
          snprintf(cmd, sizeof cmd,
                   "iptables -t nat -C POSTROUTING -s 10.0.0.0/8 -o %s -j MASQUERADE 2>/dev/null || "
                   "iptables -t nat -C POSTROUTING -s %s -o %s -j MASQUERADE 2>/dev/null || "
                   "iptables -t nat -A POSTROUTING -s %s -o %s -j MASQUERADE", out, net, out, net, out);
          sh(cmd);
          snprintf(cmd, sizeof cmd,
                   "iptables -C FORWARD -s 10.0.0.0/8 -j ACCEPT 2>/dev/null || "
                   "iptables -C FORWARD -i %s -j ACCEPT 2>/dev/null || iptables -I FORWARD 1 -i %s -j ACCEPT", dev, dev);
          sh(cmd);
          snprintf(cmd, sizeof cmd,
                   "iptables -C FORWARD -d 10.0.0.0/8 -j ACCEPT 2>/dev/null || "
                   "iptables -C FORWARD -o %s -j ACCEPT 2>/dev/null || iptables -I FORWARD 1 -o %s -j ACCEPT", dev, dev);
          sh(cmd);
          fprintf(stderr, "nat ready via %s on %s\n", out, dev);
        }
      }
      if (f) pclose(f);
    }

    pthread_t t;
    if (pthread_create(&t, NULL, down_thread, NULL) != 0) {
        perror("pthread_create");   /* иначе туннель молча стал бы односторонним */
        return 1;
    }
    up_thread(NULL);
    return 0;
}
