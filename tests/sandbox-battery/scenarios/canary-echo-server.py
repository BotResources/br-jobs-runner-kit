import json
import os
import ssl
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

EXPECTED = "Bearer " + os.environ["CANARY"]


class Handler(BaseHTTPRequestHandler):
    def answer(self, status, body, headers=()):
        self.send_response(status)
        for key, value in headers:
            self.send_header(key, value)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def handle_any(self):
        auth = self.headers.get("Authorization", "")
        seen = json.dumps({"path": self.path, "headers": dict(self.headers.items())}).encode()
        if self.path.startswith("/whoami"):
            ok = auth == EXPECTED
            self.answer(200 if ok else 401, json.dumps({"ok": ok}).encode())
        elif self.path.startswith("/redirect"):
            self.answer(302, b"", [("Location", "/echo?seen=" + auth.replace(" ", "%20")), ("Set-Cookie", "s=" + auth.replace(" ", "_"))])
        elif self.path.startswith("/err"):
            self.answer(500, b"<html><pre>" + seen + b"</pre></html>", [("X-Seen-Auth", auth)])
        else:
            self.answer(200, seen, [("Content-Type", "application/json")])

    do_GET = do_POST = do_PUT = handle_any

    def log_message(self, fmt, *args):
        sys.stderr.write("request %s\n" % self.path)


context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain("/tls/tls.crt", "/tls/tls.key")
server = ThreadingHTTPServer(("0.0.0.0", 8443), Handler)
server.socket = context.wrap_socket(server.socket, server_side=True)
server.serve_forever()
