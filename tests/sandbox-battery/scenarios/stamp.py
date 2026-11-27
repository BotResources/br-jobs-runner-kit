import sys
import time

for line in sys.stdin:
    print(int(time.time() * 1000), line.strip(), flush=True)
