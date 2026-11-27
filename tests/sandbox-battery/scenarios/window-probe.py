import socket
import sys
import threading
import time

host, port, seconds = sys.argv[1], int(sys.argv[2]), float(sys.argv[3])
start = time.monotonic()
while time.monotonic() - start < seconds:
    offset = int((time.monotonic() - start) * 1000)
    sock = socket.socket()
    sock.settimeout(0.04)
    try:
        sock.connect((host, port))
        result = "R"
    except socket.timeout:
        result = "T"
    except OSError as error:
        result = "E" + str(error.errno)
    sock.close()
    print(offset, result, flush=True)
    time.sleep(0.01)
if len(sys.argv) > 4:
    threading.Event().wait()
