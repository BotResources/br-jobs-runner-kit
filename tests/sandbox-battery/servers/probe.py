import socket
import sys
import threading
import time

PORTS = [int(port) for port in sys.argv[1:]] or [80]
METHODS = (b"GET", b"POST", b"PUT", b"HEAD", b"DELETE", b"PATCH", b"OPTIONS", b"CONNECT")
REPLY = b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nprobe\n"


def log(*parts):
    print(time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), *parts, flush=True)


def serve(connection, address, port):
    log("CONN", address[0], address[1], "port=" + str(port))
    connection.settimeout(1.0)
    try:
        data = connection.recv(4096)
        if data:
            log("DATA", address[0], "port=" + str(port), repr(data.split(b"\r\n")[0][:120]))
            if data.startswith(METHODS):
                connection.sendall(REPLY)
    except OSError:
        pass
    finally:
        connection.close()


def listen(port):
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("0.0.0.0", port))
    sock.listen(128)
    while True:
        connection, address = sock.accept()
        threading.Thread(target=serve, args=(connection, address, port), daemon=True).start()


for number in PORTS:
    threading.Thread(target=listen, args=(number,), daemon=True).start()
log("READY", *PORTS)
threading.Event().wait()
