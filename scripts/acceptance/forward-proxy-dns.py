#!/usr/bin/env python3
"""Observe emitted SOCKS5 DNS intent with curl and a loopback protocol peer.

The peer records CONNECT address types and returns a synthetic HTTP response.
It never connects to a destination. --resolve supplies a controlled local DNS
answer, so neither a real proxy nor an external DNS service is required.
"""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
from urllib.parse import urlsplit


def receive(peer, count):
    data = b""
    while len(data) < count:
        part = peer.recv(count - len(data))
        if not part:
            raise AssertionError("unexpected SOCKS peer EOF")
        data += part
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    observations, failures, commands = [], [], []
    with tempfile.TemporaryDirectory(prefix="deve-dns-contract-") as tmp, socket.socket() as listener:
        root = Path(tmp)
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        listener.settimeout(20)
        port = listener.getsockname()[1]

        def peer_loop():
            try:
                for _ in range(2):
                    with listener.accept()[0] as peer:
                        peer.settimeout(5)
                        version, methods = receive(peer, 2)
                        assert version == 5 and 0 in receive(peer, methods)
                        peer.sendall(b"\x05\x00")
                        version, command, reserved, kind = receive(peer, 4)
                        assert (version, command, reserved) == (5, 1, 0)
                        if kind == 1:
                            address = socket.inet_ntop(socket.AF_INET, receive(peer, 4))
                        elif kind == 3:
                            address = receive(peer, receive(peer, 1)[0]).decode("ascii")
                        elif kind == 4:
                            address = socket.inet_ntop(socket.AF_INET6, receive(peer, 16))
                        else:
                            raise AssertionError("invalid SOCKS address type")
                        target_port = int.from_bytes(receive(peer, 2), "big")
                        observations.append({"address_type": kind, "address": address, "port": target_port})
                        peer.sendall(b"\x05\x00\x00\x01\x7f\x00\x00\x01\x00\x50")
                        request = b""
                        while b"\r\n\r\n" not in request:
                            request += receive(peer, 1)
                            assert len(request) < 8192
                        assert request.startswith(b"GET /normal HTTP/")
                        peer.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\nsynthetic-dns-ok\n")
            except Exception as error:
                failures.append(repr(error))

        def run(arguments, data=None):
            result = subprocess.run(arguments, input=data, capture_output=True, text=True, timeout=30)
            commands.append({"command": arguments[1:3], "exit_code": result.returncode})
            assert result.returncode == 0, result.stderr
            return result.stdout

        common = ["--db-path", str(root / "nodes.db"), "--key-path", str(root / "master.key")]
        run([str(binary), "key", "init", *common])
        run([str(binary), "migrate", "--db-path", str(root / "nodes.db")])
        original = f"socks5://127.0.0.1:{port}#local\nsocks5h://127.0.0.1:{port}#remote"
        run([str(binary), "node", "import", "--source-type", "uri_list", *common], original)
        emitted = run([str(binary), "node", "list", "--format", "uri", *common])
        urls = {urlsplit(line).scheme: line.split("#")[0] for line in emitted.strip().splitlines()}
        assert set(urls) == {"socks5", "socks5h"}, "different DNS policies must retain distinct node identities"
        thread = threading.Thread(target=peer_loop)
        thread.start()
        try:
            for scheme in ("socks5", "socks5h"):
                body = run(["curl", "--silent", "--show-error", "--max-time", "5", "--noproxy", "",
                            "--proxy", urls[scheme], "--resolve", "controlled.example:80:192.0.2.55",
                            "http://controlled.example/normal"])
                assert body == "synthetic-dns-ok\n"
        finally:
            thread.join(timeout=25)
        assert not thread.is_alive() and not failures, failures
        assert observations == [
            {"address_type": 1, "address": "192.0.2.55", "port": 80},
            {"address_type": 3, "address": "controlled.example", "port": 80},
        ], observations
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {"status": "PASS", "scope": "CLI import/encrypted storage/emitted URI/curl SOCKS CONNECT; loopback synthetic peer only",
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "curl_version": subprocess.check_output(["curl", "--version"], text=True).splitlines()[0],
              "observations": observations, "commands": commands, "external_connections": False}
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
