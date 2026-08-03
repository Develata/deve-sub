#!/usr/bin/env python3
import http.server
import os
import time
import threading

PORT = 8090
DIRECTORY = os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    '..', '..', 'target', 'dx', 'dioxus-ui-spike', 'release', 'web', 'public',
)


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=DIRECTORY, **kwargs)

    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        '.wasm': 'application/wasm',
    }

    def do_GET(self):
        if self.path == '/api/progress':
            self.handle_sse()
        else:
            super().do_GET()

    def handle_sse(self):
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Cache-Control', 'no-cache')
        self.send_header('Connection', 'keep-alive')
        self.end_headers()

        for i in range(0, 101):
            data = f'data: {i}\n\n'
            try:
                self.wfile.write(data.encode())
                self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                break
            if i < 100:
                time.sleep(0.05)

    def log_message(self, format, *args):
        if '/api/progress' in str(args):
            return
        super().log_message(format, *args)


if __name__ == '__main__':
    from http.server import ThreadingHTTPServer
    print(f'Serving {DIRECTORY} on port {PORT}', flush=True)
    httpd = ThreadingHTTPServer(('127.0.0.1', PORT), Handler)
    httpd.serve_forever()
