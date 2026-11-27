import json
import ssl
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CERT = "/tls/server.crt"
KEY = "/tls/server.key"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 443


def log(*parts):
    print(time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), *parts, flush=True)


def read_chunked(stream):
    body = b""
    while True:
        size = int(stream.readline().split(b";")[0].strip() or b"0", 16)
        if size == 0:
            stream.readline()
            return body
        body += stream.read(size)
        stream.readline()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def read_body(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            return read_chunked(self.rfile)
        return self.rfile.read(int(self.headers.get("Content-Length") or 0))

    def answer(self):
        body = self.read_body()
        report = {
            "method": self.command,
            "path": self.path,
            "client": self.client_address[0],
            "headers": [[name, value] for name, value in self.headers.items()],
            "body": body.decode("utf-8", "replace"),
        }
        payload = json.dumps(report).encode()
        log("REQ", self.client_address[0], self.command, self.path, "host=" + str(self.headers.get("Host")), "bytes=" + str(len(body)))
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(payload)

    do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = do_HEAD = do_OPTIONS = answer

    def log_message(self, *args):
        return


class Server(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 128

    def get_request(self):
        connection, address = self.socket.accept()
        log("CONN", address[0], address[1])
        return connection, address

    def finish_request(self, request, client_address):
        request.settimeout(30)
        try:
            secured = CONTEXT.wrap_socket(request, server_side=True)
        except (ssl.SSLError, OSError) as error:
            log("TLSFAIL", client_address[0], type(error).__name__)
            return
        super().finish_request(secured, client_address)

    def handle_error(self, request, client_address):
        log("ERROR", client_address[0], sys.exc_info()[0].__name__)


def sni_seen(_sock, name, _context):
    log("SNI", name)


CONTEXT = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
CONTEXT.load_cert_chain(CERT, KEY)
CONTEXT.sni_callback = sni_seen

server = Server(("0.0.0.0", PORT), Handler)
log("READY", PORT)
server.serve_forever()
