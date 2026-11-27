import socketserver
import struct
import sys
import threading
import time

RECORDS = {}
TTL = int(sys.argv[2]) if len(sys.argv) > 2 else 5


def log(*parts):
    print(time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), *parts, flush=True)


def load(path):
    with open(path) as handle:
        for line in handle:
            fields = line.split()
            if len(fields) == 2:
                RECORDS[fields[0].lower().rstrip(".")] = [int(part) for part in fields[1].split(".")]


def parse_question(packet):
    position = 12
    labels = []
    while packet[position]:
        length = packet[position]
        labels.append(packet[position + 1 : position + 1 + length].decode("ascii", "replace"))
        position += 1 + length
    position += 1
    qtype, _qclass = struct.unpack("!HH", packet[position : position + 4])
    return ".".join(labels).lower(), qtype, packet[12 : position + 4]


def answer(packet, client):
    if len(packet) < 17:
        return None
    ident, flags, count = struct.unpack("!HHH", packet[:6])
    if flags & 0x8000 or count != 1:
        return None
    try:
        name, qtype, question = parse_question(packet)
    except (IndexError, struct.error):
        return None
    known = name in RECORDS
    rcode = 0 if known else 3
    reply = b""
    if known and qtype == 1:
        reply = b"\xc0\x0c" + struct.pack("!HHIH", 1, 1, TTL, 4) + bytes(RECORDS[name])
    log("Q", client, name, qtype, "rcode=" + str(rcode), "answers=" + str(1 if reply else 0))
    head = struct.pack("!HHHHHH", ident, 0x8400 | (flags & 0x0100) | rcode, 1, 1 if reply else 0, 0, 0)
    return head + question + reply


class Udp(socketserver.BaseRequestHandler):
    def handle(self):
        data, sock = self.request
        response = answer(data, self.client_address[0])
        if response:
            sock.sendto(response, self.client_address)


class Tcp(socketserver.BaseRequestHandler):
    def handle(self):
        self.request.settimeout(5)
        try:
            while True:
                header = self.request.recv(2)
                if len(header) < 2:
                    return
                size = struct.unpack("!H", header)[0]
                data = b""
                while len(data) < size:
                    chunk = self.request.recv(size - len(data))
                    if not chunk:
                        return
                    data += chunk
                response = answer(data, self.client_address[0])
                if response:
                    self.request.sendall(struct.pack("!H", len(response)) + response)
        except OSError:
            return


class UdpServer(socketserver.ThreadingUDPServer):
    daemon_threads = True


class TcpServer(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


load(sys.argv[1])
threading.Thread(target=TcpServer(("0.0.0.0", 53), Tcp).serve_forever, daemon=True).start()
log("READY", 53, sorted(RECORDS))
UdpServer(("0.0.0.0", 53), Udp).serve_forever()
