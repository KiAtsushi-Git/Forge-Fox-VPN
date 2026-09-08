import paramiko
import time
import sys

host = '89.125.188.18'
user = 'root'
password = 'P5B7uyMs8d9Uf'

print(f"Connecting to {host}...")
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
try:
    client.connect(hostname=host, username=user, password=password, timeout=10)
    print("Connected successfully!")
    
    commands = [
        "sysctl -w net.ipv4.ip_forward=1",
        "echo 'net.ipv4.ip_forward=1' > /etc/sysctl.d/99-forgefox-vpn.conf",
        "sysctl -p /etc/sysctl.d/99-forgefox-vpn.conf",
        "modprobe tun",
        "echo 'tun' > /etc/modules-load.d/tun.conf",
        "PRIMARY_IF=$(ip route show default | awk '/default/ {print $5}'); iptables -t nat -D POSTROUTING -s 10.0.0.0/24 -o $PRIMARY_IF -j MASQUERADE 2>/dev/null",
        "PRIMARY_IF=$(ip route show default | awk '/default/ {print $5}'); iptables -t nat -A POSTROUTING -s 10.0.0.0/24 -o $PRIMARY_IF -j MASQUERADE",
        "if command -v netfilter-persistent &> /dev/null; then netfilter-persistent save; elif command -v iptables-save &> /dev/null; then iptables-save > /etc/iptables/rules.v4; fi",
        "sed -i 's/^#PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config",
        "sed -i 's/^PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config",
        "sed -i 's/^#PermitTunnel.*/PermitTunnel yes/' /etc/ssh/sshd_config",
        "if ! grep -q '^PermitTunnel' /etc/ssh/sshd_config; then echo 'PermitTunnel yes' >> /etc/ssh/sshd_config; fi",
        "systemctl restart sshd || systemctl restart ssh"
    ]
    
    for cmd in commands:
        print(f"Running: {cmd}")
        stdin, stdout, stderr = client.exec_command(cmd)
        exit_status = stdout.channel.recv_exit_status()
        print(f"Stdout: {stdout.read().decode().strip()}")
        err = stderr.read().decode().strip()
        if err:
            print(f"Stderr: {err}")
            
    print("Server setup complete!")
except Exception as e:
    print(f"Error: {e}")
finally:
    client.close()
