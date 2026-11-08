"""Records what the real PostgreSQL server of tests/docker answers to two statements, as hex, into
tests/data/postgres-exchange.txt. The replay test in tests/postgres.rs plays these bytes back to
the Zila Lang driver, so the real message framing and type OIDs are checked without Docker.

    docker compose -f tests/docker/compose.yml up -d --wait
    python tests/docker/record_postgres.py

Line format: `<name> <hex of everything the server sent from our Sync until ReadyForQuery>`.
"""
import base64, hashlib, hmac, os, socket, ssl, struct, sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "data", "postgres-exchange.txt")
USER, PASSWORD, DB = "app", "Zila_app_dev_1", "shop"


def frame(kind, body):
    return kind + struct.pack(">I", len(body) + 4) + body


def read_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise EOFError
        buf += chunk
    return buf


def read_msg(sock, raw=None):
    head = read_exact(sock, 5)
    body = read_exact(sock, struct.unpack(">I", head[1:])[0] - 4)
    if raw is not None:
        raw.append(head + body)
    return head[:1], body


def connect():
    s = socket.create_connection(("127.0.0.1", 55432))
    s.sendall(struct.pack(">II", 8, 80877103))
    assert s.recv(1) == b"S"
    ctx = ssl.create_default_context(cafile=os.path.join(HERE, "certs", "ca.pem"))
    s = ctx.wrap_socket(s, server_hostname="localhost")
    params = b"user\0" + USER.encode() + b"\0database\0" + DB.encode() + b"\0client_encoding\0UTF8\0\0"
    s.sendall(struct.pack(">II", len(params) + 8, 196608) + params)
    kind, body = read_msg(s)
    assert kind == b"R" and struct.unpack(">I", body[:4])[0] == 10, "expected SASL"
    nonce = base64.b64encode(os.urandom(18)).decode()
    first_bare = "n=,r=" + nonce
    init = ("n,," + first_bare).encode()
    s.sendall(frame(b"p", b"SCRAM-SHA-256\0" + struct.pack(">I", len(init)) + init))
    kind, body = read_msg(s)
    server_first = body[4:].decode()
    parts = dict(p.split("=", 1) for p in server_first.split(","))
    salted = hashlib.pbkdf2_hmac("sha256", PASSWORD.encode(), base64.b64decode(parts["s"]), int(parts["i"]))
    client_key = hmac.new(salted, b"Client Key", "sha256").digest()
    stored = hashlib.sha256(client_key).digest()
    without = "c=biws,r=" + parts["r"]
    auth = first_bare + "," + server_first + "," + without
    sig = hmac.new(stored, auth.encode(), "sha256").digest()
    proof = bytes(a ^ b for a, b in zip(client_key, sig))
    s.sendall(frame(b"p", (without + ",p=" + base64.b64encode(proof).decode()).encode()))
    while True:
        kind, body = read_msg(s)
        if kind == b"Z":
            return s


def cstr(x):
    return x.encode() + b"\0"


def run(sock, sql, params):
    bind = b"\0\0" + struct.pack(">HH", 0, len(params))
    for p in params:
        bind += struct.pack(">i", len(p)) + p.encode()
    bind += struct.pack(">H", 0)
    sock.sendall(
        frame(b"P", b"\0" + cstr(sql) + struct.pack(">H", 0))
        + frame(b"B", bind)
        + frame(b"D", b"P\0")
        + frame(b"E", b"\0" + struct.pack(">I", 0))
        + frame(b"S", b"")
    )
    raw = []
    while True:
        kind, _ = read_msg(sock, raw)
        if kind == b"Z":
            return b"".join(raw)


def main():
    s = connect()
    lines = []
    lines.append("customers " + run(s, "select id, name, email, city from customers where id <= $1 order by id", ["4"]).hex())
    lines.append("types " + run(s, "select true as b, 42::int8 as n, 1.5::float8 as f, 12.50::numeric as d, 'x'::text as t, '\\xdead'::bytea as y, null::text as z, now()::date as day", []).hex())
    lines.append("boom " + run(s, "insert into customers(id, name) values (1, 'dup')", []).hex())
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote", OUT)


if __name__ == "__main__":
    sys.exit(main())
