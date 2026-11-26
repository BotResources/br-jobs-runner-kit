import os
import sys
import time
import urllib.error
import urllib.request

HOST = sys.argv[1]
HOLD_SECONDS = int(sys.argv[2])
PATHS = sys.argv[3:]
TOKEN = os.environ[os.environ["CANARY_PLACEHOLDER_ENV_NAME"]]
OUT = "/work/canary-out"


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


opener = urllib.request.build_opener(
    urllib.request.ProxyHandler({"https": os.environ["HTTPS_PROXY"]}), NoRedirect
)
os.makedirs(OUT, exist_ok=True)
held = []
for index, path in enumerate(PATHS):
    request = urllib.request.Request(
        "https://%s%s" % (HOST, path), headers={"Authorization": "Bearer " + TOKEN}
    )
    try:
        response = opener.open(request, timeout=15)
        status, headers, body = response.status, dict(response.headers), response.read()
    except urllib.error.HTTPError as error:
        status, headers, body = error.code, dict(error.headers), error.read()
    held.append((status, headers, body))
    with open("%s/%d.txt" % (OUT, index), "w") as handle:
        handle.write("%s %s\n%s\n%s\n" % (path, status, headers, body.decode(errors="replace")))
    print(path, status)
time.sleep(HOLD_SECONDS)
