import socket
import sys
import time

host, port, seconds = sys.argv[1], int(sys.argv[2]), float(sys.argv[3])
end = time.time() + seconds
while time.time() < end:
    sock = socket.socket()
    sock.settimeout(0.02)
    try:
        sock.connect((host, port))
        result = 1
    except OSError:
        result = 0
    sock.close()
    print(result, flush=True)
    time.sleep(0.02)
