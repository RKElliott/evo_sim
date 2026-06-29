#!/usr/bin/env python3
# evo_sim - Copyright (c) 2026 Lens and Mix, LLC
# Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
# More information: https://rkeithelliott.com
"""
Development server with COOP/COEP headers required for SharedArrayBuffer.
SharedArrayBuffer enables zero-copy shared memory between main thread and Web Worker.

Usage: python server.py [port]
Default port: 9090
"""
import sys
import os
from http.server import HTTPServer, SimpleHTTPRequestHandler

class COEPHandler(SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy',   'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        # Dev server: never cache. Prevents the browser (and module workers,
        # which fetch their own WASM imports) from serving stale assets after a
        # rebuild — the silent-black-canvas trap.
        self.send_header('Cache-Control', 'no-store')
        super().end_headers()

    def log_message(self, fmt, *args):
        # Suppress noisy asset logs. Guard: error logs (send_error) pass an
        # HTTPStatus code as args[0], not a request-line string, so calling
        # .endswith() on it crashed the handler.
        if args and isinstance(args[0], str) and any(
                args[0].endswith(ext) for ext in ('.js', '.wasm', '.css', '.png', '.ico')):
            return
        super().log_message(fmt, *args)

if __name__ == '__main__':
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9090
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    server = HTTPServer(('', port), COEPHandler)
    print(f'Evo Sim server running at http://localhost:{port}')
    print(f'Serving from: {os.getcwd()}')
    print(f'Press Ctrl+C to stop')
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print('\nServer stopped')
