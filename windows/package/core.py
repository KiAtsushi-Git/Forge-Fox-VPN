import os
import re
import json
import random
import string
import subprocess
import time
import socket
from urllib.parse import urlparse, parse_qs
from cryptography.fernet import Fernet

from PyQt6.QtCore import QThread, pyqtSignal

from package.utils import resource_path

class ObfuscatorThread(QThread):
    def __init__(self, sites):
        super().__init__()
        self.sites = sites
        self.is_running = True

    def run(self):
        import urllib.request
        import random
        import time

        time.sleep(5)

        while self.is_running:
            site = random.choice(self.sites).strip()
            url = f"https://{site}" if not site.startswith("http") else site
            try:
                req = urllib.request.Request(
                    url,
                    headers={
                        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36'}
                )
                urllib.request.urlopen(req, timeout=5)
            except Exception:
                pass

            for _ in range(random.randint(15, 45)):
                if not self.is_running:
                    break
                time.sleep(1)

    def stop(self):
        self.is_running = False

class SecureStorage:
    def __init__(self, app_name="ForgeFoxVPN", data_file="servers.dat"):
        if os.name == 'nt':
            self.app_dir = os.path.join(os.getenv('APPDATA'), app_name)
        else:
            self.app_dir = os.path.join(os.path.expanduser('~'), '.config', app_name)

        os.makedirs(self.app_dir, exist_ok=True)
        self.key_file = os.path.join(self.app_dir, "forge.key")
        self.data_file = os.path.join(self.app_dir, data_file)
        self.config_file = os.path.join(self.app_dir, "config.json")
        self.cipher = self._init_cipher()

    def _init_cipher(self):
        if not os.path.exists(self.key_file):
            key = Fernet.generate_key()
            with open(self.key_file, "wb") as f:
                f.write(key)
        else:
            with open(self.key_file, "rb") as f:
                key = f.read()
        return Fernet(key)

    def save(self, data):
        with open(self.data_file, "wb") as f:
            f.write(self.cipher.encrypt(json.dumps(data).encode('utf-8')))

    def load(self):
        if not os.path.exists(self.data_file): return []
        try:
            with open(self.data_file, "rb") as f:
                enc = f.read()
            return json.loads(self.cipher.decrypt(enc).decode('utf-8'))
        except Exception:
            return []

class VPNManager:
    @staticmethod
    def parse_vless(link: str):
        u = urlparse(link)
        params = parse_qs(u.query)
        return {
            "uuid": u.username, "server": u.hostname, "port": u.port,
            "flow": params.get("flow", [""])[0],
            "sni": params.get("sni", [""])[0],
            "fp": params.get("fp", ["chrome"])[0],
            "pbk": params.get("pbk", [""])[0],
            "sid": params.get("sid", [""])[0],
            "type": params.get("type", ["tcp"])[0]
        }

    @staticmethod
    def build_config(v, mode, proxy_user="Fox", proxy_pass="Forge", proxy_port=1080, bypass_domains=None,
                     bypass_apps=None, split_mode=0, adblock_enabled=False,
                     adblock_url="https://raw.githubusercontent.com/Dreista/sing-box-rule-set-cn/rule-set/filter.txt.srs",
                     split_enabled=False, detour_v=None):

        if bypass_domains is None: bypass_domains = []
        if bypass_apps is None: bypass_apps = []

        if not split_enabled:
            target_outbound = "direct"
            final_outbound = "proxy" if mode != "adblock" else "direct"
            bypass_domains = []
            bypass_apps = []
        else:
            target_outbound = "direct" if split_mode == 0 else "proxy"
            final_outbound = "proxy" if split_mode == 0 else "direct"

        if mode == "adblock":
            final_outbound = "direct"

        domains = []
        ips = []
        for d in bypass_domains:
            d = d.strip()
            if not d: continue
            if re.match(r'^[\d\./]+$', d) or ':' in d:
                ips.append(d)
            else:
                domains.append(d.lstrip('.'))

        rules = [
            {"action": "sniff"},
            {"protocol": "dns", "action": "hijack-dns"}
        ]

        if adblock_enabled or mode == "adblock":
            rules.append({"rule_set": ["geosite-ads"], "outbound": "block"})

        if domains and mode != "adblock":
            rules.append({"domain_suffix": domains, "outbound": target_outbound})
        if ips and mode != "adblock":
            rules.append({"ip_cidr": ips, "outbound": target_outbound})
        if bypass_apps and mode != "adblock":
            smart_apps = []
            for app in bypass_apps:
                smart_apps.extend([app, app.lower(), app.capitalize()])
            rules.append({"process_name": list(set(smart_apps)), "outbound": target_outbound})

        rules.append({"action": "resolve"})

        dns_remote_server = {"type": "https", "tag": "dns-remote", "server": "8.8.8.8",
                             "tls": {"server_name": "dns.google"}}
        if mode != "adblock":
            dns_remote_server["detour"] = "proxy"

        base = {
            "log": {"level": "info"},
            "dns": {"strategy": "ipv4_only", "servers": [
                dns_remote_server,
                {"type": "udp", "tag": "dns-direct", "server": "8.8.8.8", "server_port": 53}],
                    "final": "dns-remote"},
            "outbounds": [
                {"type": "direct", "tag": "direct"},
                {"type": "block", "tag": "block"}
            ],
            "route": {"auto_detect_interface": True, "final": final_outbound,
                      "default_domain_resolver": {"server": "dns-remote"}, "rules": rules}
        }

        if mode != "adblock" and v:
            main_proxy = {
                "type": "vless", "tag": "proxy", "server": v["server"], "server_port": v["port"], "uuid": v["uuid"],
                "flow": v["flow"], "packet_encoding": "xudp",
                "tls": {"enabled": True, "server_name": v["sni"], "utls": {"enabled": True, "fingerprint": v["fp"]},
                        "reality": {"enabled": True, "public_key": v["pbk"], "short_id": v["sid"]}}
            }

            if detour_v:
                main_proxy["detour"] = "proxy-entry"

                entry_proxy = {
                    "type": "vless", "tag": "proxy-entry", "server": detour_v["server"],
                    "server_port": detour_v["port"], "uuid": detour_v["uuid"],
                    "flow": detour_v["flow"], "packet_encoding": "xudp",
                    "tls": {"enabled": True, "server_name": detour_v["sni"],
                            "utls": {"enabled": True, "fingerprint": detour_v["fp"]},
                            "reality": {"enabled": True, "public_key": detour_v["pbk"], "short_id": detour_v["sid"]}}
                }
                base["outbounds"].insert(0, entry_proxy)

            base["outbounds"].insert(0, main_proxy)

        if adblock_enabled or mode == "adblock":
            base["route"]["rule_set"] = [{
                "tag": "geosite-ads", "type": "remote", "format": "binary", "url": adblock_url,
                "download_detour": "proxy" if mode != "adblock" else "direct"
            }]

        if mode in ["tunnel", "adblock"]:
            iface_name = "ForgeFox-" + ''.join(random.choices(string.ascii_uppercase + string.digits, k=4))
            subnet = random.randint(10, 250)
            base["inbounds"] = [
                {"type": "tun", "tag": "tun-in", "interface_name": iface_name, "address": [f"172.19.{subnet}.1/30"],
                 "mtu": 1500, "auto_route": True, "strict_route": True, "stack": "system"}]
        else:
            base["inbounds"] = [{"type": "socks", "tag": "socks-in", "listen": "127.0.0.1",
                                 "listen_port": int(proxy_port),
                                 "users": [{"username": proxy_user, "password": proxy_pass}]}]
        return base

class VPNThread(QThread):
    log_signal = pyqtSignal(str)
    process_died_signal = pyqtSignal()

    def __init__(self, config_data):
        super().__init__()
        self.process = None
        self.is_running = True
        self.config_data = config_data

    def run(self):
        CREATE_NO_WINDOW = 0x08000000

        subprocess.run(['taskkill', '/F', '/IM', 'sing-box.exe'],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       creationflags=CREATE_NO_WINDOW)

        sing_box_path = resource_path("./sing-box/sing-box.exe")

        self.process = subprocess.Popen(
            [sing_box_path, "run", "-c", "stdin"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            creationflags=CREATE_NO_WINDOW
        )

        config_json = json.dumps(self.config_data)
        try:
            self.process.stdin.write(config_json)
            self.process.stdin.flush()
            self.process.stdin.close()
        except Exception as e:
            self.log_signal.emit(f"[!] Ошибка записи в RAM: {e}")
            return

        while self.is_running and self.process.poll() is None:
            try:
                line = self.process.stdout.readline()
                if line:
                    self.log_signal.emit(line.strip())
                elif self.process.poll() is not None:
                    break
            except Exception:
                break

        if self.is_running:
            self.process_died_signal.emit()

    def stop(self):
        self.is_running = False
        if self.process:
            CREATE_NO_WINDOW = 0x08000000
            subprocess.run(['taskkill', '/F', '/T', '/PID', str(self.process.pid)],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           creationflags=CREATE_NO_WINDOW)
            self.process = None

class NetworkTester(QThread):
    result_signal = pyqtSignal(str, str, int)

    def __init__(self, test_type, server_dict):
        super().__init__()
        self.test_type = test_type
        self.server_dict = server_dict
        self.server_name = server_dict["name"]
        try:
            self.v = VPNManager.parse_vless(server_dict["link"])
            self.host = self.v.get("server")
            self.port = int(self.v.get("port", 443))
            self.sni = self.v.get("sni") or self.host
        except:
            self.host = None

    def run(self):
        if not self.host:
            self.result_signal.emit(self.test_type, self.server_name, -1)
            return

        if self.test_type == "ping":
            self._test_tcp()
        elif self.test_type == "get":
            self._test_real_ping()

    def _test_tcp(self):
        try:
            start = time.perf_counter()
            with socket.create_connection((self.host, self.port), timeout=3):
                pass
            ms = max(1, int((time.perf_counter() - start) * 1000))
            self.result_signal.emit("ping", self.server_name, ms)
        except:
            self.result_signal.emit("ping", self.server_name, -1)

    def _test_real_ping(self):
        import urllib.request
        port = random.randint(20000, 50000)

        config = {
            "log": {"level": "fatal"},
            "inbounds": [{"type": "http", "tag": "http-in", "listen": "127.0.0.1", "listen_port": port}],
            "outbounds": [
                {
                    "type": "vless", "tag": "proxy", "server": self.host, "server_port": self.port,
                    "uuid": self.v.get("uuid"), "flow": self.v.get("flow", ""), "packet_encoding": "xudp",
                    "tls": {"enabled": True, "server_name": self.v.get("sni"),
                            "utls": {"enabled": True, "fingerprint": self.v.get("fp", "chrome")},
                            "reality": {"enabled": bool(self.v.get("pbk")), "public_key": self.v.get("pbk", ""),
                                        "short_id": self.v.get("sid", "")}}
                },
                {"type": "direct", "tag": "direct"}
            ]
        }

        CREATE_NO_WINDOW = 0x08000000
        process = None
        try:
            process = subprocess.Popen(
                [resource_path("./sing-box/sing-box.exe"), "run", "-c", "stdin"],
                stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                text=True, creationflags=CREATE_NO_WINDOW
            )
            process.stdin.write(json.dumps(config))
            process.stdin.flush()
            process.stdin.close()

            time.sleep(0.3)

            proxy_handler = urllib.request.ProxyHandler({'http': f'http://127.0.0.1:{port}'})
            opener = urllib.request.build_opener(proxy_handler)

            start_time = time.time()
            req = urllib.request.Request("http://www.gstatic.com/generate_204", headers={'User-Agent': 'Mozilla/5.0'})
            resp = opener.open(req, timeout=3)

            if resp.getcode() == 204:
                ms = int((time.time() - start_time) * 1000)
                self.result_signal.emit("get", self.server_name, ms)
            else:
                self.result_signal.emit("get", self.server_name, -1)
        except Exception:
            self.result_signal.emit("get", self.server_name, -1)
        finally:
            if process:
                process.terminate()
                process.wait(timeout=1)
