from __future__ import annotations

import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class ExportHandler(SimpleHTTPRequestHandler):
    server_version = "SQLiteCapsuleAcceptance/0.2"

    def end_headers(self) -> None:
        self.send_header("Cache-Control", "no-store")
        if getattr(self.server, "cross_origin_isolated", False):
            self.send_header("Cross-Origin-Opener-Policy", "same-origin")
            self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
            self.send_header("Cross-Origin-Resource-Policy", "same-origin")
        super().end_headers()

    def log_message(self, format: str, *args: object) -> None:
        return


def main() -> None:
    parser = argparse.ArgumentParser(description="Loopback-only static server for HTML export acceptance")
    parser.add_argument("--port", required=True, type=int)
    parser.add_argument("--directory", required=True, type=Path)
    parser.add_argument("--cross-origin-isolated", action="store_true")
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error("--port must be between 1 and 65535")
    directory = args.directory.resolve(strict=True)
    handler = partial(ExportHandler, directory=str(directory))
    server = ThreadingHTTPServer(("127.0.0.1", args.port), handler)
    server.cross_origin_isolated = args.cross_origin_isolated  # type: ignore[attr-defined]
    server.serve_forever()


if __name__ == "__main__":
    main()
