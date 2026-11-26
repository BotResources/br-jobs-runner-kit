import json
import os
import socket
import sys
from concurrent.futures import ThreadPoolExecutor


def tcp_open(item):
    try:
        socket.create_connection((item["host"], int(item["port"])), 2.0).close()
        return {**item, "open": True}
    except OSError:
        return {**item, "open": False}


def resolves(item):
    try:
        socket.getaddrinfo(item["name"], None)
        return {**item, "resolved": True}
    except OSError:
        return {**item, "resolved": False}


def connect_status(item):
    host, port = item["proxy"].rsplit(":", 1)
    request = "CONNECT {0} HTTP/1.1\r\nHost: {0}\r\n\r\n".format(item["target"])
    try:
        sock = socket.create_connection((host, int(port)), 5.0)
        sock.sendall(request.encode())
        status_line = sock.recv(256).split(b"\r\n")[0].decode(errors="replace")
        sock.close()
        return {**item, "status": int(status_line.split()[1])}
    except (OSError, ValueError, IndexError):
        return {**item, "status": 0}


MODES = {"tcp": tcp_open, "names": resolves, "connect": connect_status}

with ThreadPoolExecutor(16) as pool:
    for result in pool.map(MODES[sys.argv[1]], json.loads(os.environ["PROBE_INPUT"])):
        print(json.dumps(result))
