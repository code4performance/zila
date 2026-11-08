"""Records what the real SQL Server container (plain port 51433) answers to the client program
tests/data/sqlserver-client.br, as hex, into tests/data/sqlserver-exchange.txt. The replay test in
tests/sqlserver.rs plays the same replies back, one per request message, to the Zila Lang driver, so the
real token streams are checked without Docker.

    docker compose -f tests/docker/compose.yml up -d --wait
    python tests/docker/record_sqlserver.py

A TCP proxy on 127.0.0.1:51600 sits between the compiled client and the server.
"""
import os, socket, subprocess, sys, tempfile, threading

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
CLIENT = os.path.join(ROOT, "tests", "data", "sqlserver-client.br")
OUT = os.path.join(ROOT, "tests", "data", "sqlserver-exchange.txt")
PORT = 51600
replies = []


def pump(src, dst, record):
    """Copies bytes; with `record`, groups them into TDS messages (until an EOM packet)."""
    buf = b""
    msg = b""
    try:
        while True:
            chunk = src.recv(65536)
            if not chunk:
                break
            dst.sendall(chunk)
            if record:
                buf += chunk
                while len(buf) >= 8:
                    length = int.from_bytes(buf[2:4], "big")
                    if len(buf) < length:
                        break
                    packet, buf = buf[:length], buf[length:]
                    msg += packet
                    if packet[1] & 1:
                        replies.append(msg)
                        msg = b""
    except OSError:
        pass
    finally:
        for s in (src, dst):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


def proxy(listener):
    client, _ = listener.accept()
    server = socket.create_connection(("127.0.0.1", 51433))
    a = threading.Thread(target=pump, args=(client, server, False))
    b = threading.Thread(target=pump, args=(server, client, True))
    a.start(); b.start(); a.join(); b.join()


def main():
    exe = os.path.join(tempfile.mkdtemp(), "rec.exe")
    src = open(CLIENT, encoding="utf-8").read().replace("@PORT@", str(PORT))
    br = os.path.join(os.path.dirname(exe), "rec.br")
    open(br, "w", encoding="utf-8").write(src)
    compiler = os.path.join(ROOT, "target", "debug", "zila-parse.exe")
    subprocess.run([compiler, br, "-o", exe], check=True)
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", PORT))
    listener.listen(1)
    t = threading.Thread(target=proxy, args=(listener,))
    t.start()
    out = subprocess.run([exe], capture_output=True, text=True)
    t.join(timeout=10)
    print(out.stdout)
    with open(OUT, "w", newline="\n") as f:
        f.write("\n".join(r.hex() for r in replies) + "\n")
    print("recorded", len(replies), "replies to", OUT)


if __name__ == "__main__":
    sys.exit(main())
