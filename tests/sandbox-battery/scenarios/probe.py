import json
import os
import socket
import ssl
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
    except (OSError, ValueError, IndexError) as error:
        return {**item, "status": 0, "error": type(error).__name__}


def first_line(sock):
    try:
        return sock.recv(512).split(b"\r\n")[0].decode(errors="replace")
    except OSError as error:
        return "read-error:" + type(error).__name__


def request_for(item):
    host = item["target"].rsplit(":", 1)[0]
    return "GET / HTTP/1.1\r\nHost: {0}\r\nConnection: close\r\n\r\n".format(host).encode()


def inner_exchange(sock, item):
    if item.get("tls"):
        context = ssl.create_default_context(cafile=os.environ.get("SSL_CERT_FILE"))
        if not item.get("sni"):
            context.check_hostname = False
        try:
            sock = context.wrap_socket(sock, server_hostname=item.get("sni"))
        except (OSError, ssl.SSLError) as error:
            return "tls-error:" + type(error).__name__
    try:
        sock.sendall(request_for(item))
    except OSError as error:
        return "write-error:" + type(error).__name__
    return first_line(sock)


def through_proxy(item):
    host, port = item["proxy"].rsplit(":", 1)
    try:
        sock = socket.create_connection((host, int(port)), 5.0)
        sock.settimeout(8.0)
        if item.get("absolute"):
            sock.sendall("GET http://{0}/ HTTP/1.1\r\nHost: {0}\r\nConnection: close\r\n\r\n".format(item["target"]).encode())
            return {**item, "status": 0, "inner": first_line(sock)}
        sock.sendall("CONNECT {0} HTTP/1.1\r\nHost: {0}\r\n\r\n".format(item["target"]).encode())
        status_line = first_line(sock)
        status = int(status_line.split()[1]) if status_line.startswith("HTTP/") else 0
        inner = inner_exchange(sock, item) if status == 200 else ""
        sock.close()
        return {**item, "status": status, "inner": inner}
    except (OSError, ValueError, IndexError) as error:
        return {**item, "status": 0, "inner": "error:" + type(error).__name__}


MODES = {"tcp": tcp_open, "names": resolves, "connect": connect_status, "via": through_proxy}

with ThreadPoolExecutor(16) as pool:
    for result in pool.map(MODES[sys.argv[1]], json.loads(os.environ["PROBE_INPUT"])):
        print(json.dumps(result))
