import socket
import struct
import sys

server, name = sys.argv[1], sys.argv[2]
labels = b"".join(bytes([len(part)]) + part.encode() for part in name.split("."))
query = struct.pack("!HHHHHH", 0x5B58, 0x0100, 1, 0, 0, 0) + labels + b"\x00" + struct.pack("!HH", 1, 1)
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(3)
sock.sendto(query, (server, 53))
data, _ = sock.recvfrom(2048)
rcode = struct.unpack("!H", data[2:4])[0] & 0x0F
count = struct.unpack("!H", data[6:8])[0]
position = 12 + len(labels) + 1 + 4
ips = []
for _ in range(count):
    position += 2
    rtype, _rclass, _ttl, length = struct.unpack("!HHIH", data[position : position + 10])
    position += 10
    if rtype == 1:
        ips.append(".".join(str(byte) for byte in data[position : position + 4]))
    position += length
print("rcode=%d ips=%s" % (rcode, ",".join(ips)))
