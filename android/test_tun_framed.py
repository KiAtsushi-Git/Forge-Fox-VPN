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
 def rx(f,n):
  b=b''
  while len(b)<n:
   c=f.read(n-len(b))
   if not c:return None
   b+=c
  return b
 while True:
  r,_,_=select.select([sys.stdin.buffer,tun],[],[])
  if sys.stdin.buffer in r:
   lb=rx(sys.stdin.buffer,2)
   if not lb:break
   l,=struct.unpack('!H',lb)
   p=rx(sys.stdin.buffer,l)
   if not p:break
   tun.write(p)
  if tun in r:
   p=tun.read(4096)
   sys.stdout.buffer.write(struct.pack('!H',len(p))+p)
   sys.stdout.buffer.flush()
except Exception as e:
 pass
"
"""
    print('Sending script...')
    stdin, stdout, stderr = ssh.exec_command(python_script)
    time.sleep(1)
    
    # Send a dummy framed packet (len=4, data='PING')
    stdin.write(b'\x00\x04PING')
    stdin.flush()
    time.sleep(1)
    
    print('STDOUT:', stdout.channel.recv(4096))
    print('STDERR:', stderr.read().decode().strip())
except Exception as e:
    print('Error:', e)
