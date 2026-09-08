import paramiko
import time

ssh = paramiko.SSHClient()
ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
try:
    ssh.connect('89.125.126.57', port=22, username='root', password='wTgkkDzj2n36A')
    
    python_script = """python3 -c "
import os,struct,fcntl,sys,select
try:
 tun=open('/dev/net/tun','r+b',buffering=0)
 fcntl.ioctl(tun,0x400454ca,struct.pack('16sH',b'tun0',0x1001))
 os.system('ip link set tun0 up && ip addr add 10.0.0.1/24 dev tun0')
 print('TUN initialized successfully')
except Exception as e:
 print('Error:', e)
"
"""
    stdin, stdout, stderr = ssh.exec_command(python_script)
    time.sleep(1)
    print('STDOUT:', stdout.read().decode().strip())
    print('STDERR:', stderr.read().decode().strip())
except Exception as e:
    print('Error:', e)
