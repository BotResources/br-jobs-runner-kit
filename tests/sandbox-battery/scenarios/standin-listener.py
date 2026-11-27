import socket
import sys
import threading
import time


def serve(port):
    server = socket.socket()
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("0.0.0.0", port))
    server.listen(16)
    while True:
        server.accept()[0].close()


for value in sys.argv[1:]:
    threading.Thread(target=serve, args=(int(value),), daemon=True).start()
time.sleep(3600)
