#!/bin/bash

echo "Starting ForgeFox VPN Server Setup..."

# Ensure we are root
if [ "$EUID" -ne 0 ]; then 
  echo "Please run as root (use sudo)"
  exit
fi

# Enable IP forwarding
echo "Enabling IP forwarding..."
sysctl -w net.ipv4.ip_forward=1
echo "net.ipv4.ip_forward=1" > /etc/sysctl.d/99-forgefox-vpn.conf
sysctl -p /etc/sysctl.d/99-forgefox-vpn.conf

# Load TUN module
echo "Loading TUN module..."
modprobe tun
echo "tun" > /etc/modules-load.d/tun.conf

# Setup NAT (iptables)
echo "Setting up NAT rules..."
# Find primary internet interface
PRIMARY_IF=$(ip route show default | awk '/default/ {print $5}')
echo "Primary interface detected as $PRIMARY_IF"

# Flush old rules to be safe
iptables -t nat -D POSTROUTING -s 10.0.0.0/8 -o $PRIMARY_IF -j MASQUERADE 2>/dev/null
iptables -t nat -D POSTROUTING -s 10.0.0.0/24 -o $PRIMARY_IF -j MASQUERADE 2>/dev/null
iptables -t nat -A POSTROUTING -s 10.0.0.0/8 -o $PRIMARY_IF -j MASQUERADE

# Save iptables
echo "Saving iptables..."
if command -v netfilter-persistent &> /dev/null; then
    netfilter-persistent save
elif command -v iptables-save &> /dev/null; then
    if [ -d /etc/iptables ]; then
        iptables-save > /etc/iptables/rules.v4
    fi
fi

# Configure SSH daemon
echo "Configuring SSH..."
SSHD_CONFIG="/etc/ssh/sshd_config"

sed -i 's/^#PermitRootLogin.*/PermitRootLogin yes/' $SSHD_CONFIG
sed -i 's/^PermitRootLogin.*/PermitRootLogin yes/' $SSHD_CONFIG

sed -i 's/^#PermitTunnel.*/PermitTunnel yes/' $SSHD_CONFIG
if ! grep -q "^PermitTunnel" $SSHD_CONFIG; then
    echo "PermitTunnel yes" >> $SSHD_CONFIG
fi

# Restart SSH service
echo "Restarting SSH service..."
systemctl restart sshd || systemctl restart ssh
systemctl restart ssh.socket

echo "======================================"
echo "ForgeFox VPN Server Setup Complete!"
echo "Server is ready to accept SSH VPN connections."
echo "======================================"
