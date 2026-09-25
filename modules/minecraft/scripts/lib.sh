# Shared helpers, loaded before every script.
# Needs: MC_DIR, SCREEN_DIR, SCREEN_USER, SCREEN_SESSION.

# Type one line into the server console.
# Not `screen -X stuff` (what mc-cmd uses): stuff interprets ^M, \015 and $VARS in its
# argument, so "say hi^Mop someone" would run two commands. readbuf + paste is literal.
console_send() {
  local buf
  buf=$(mktemp /tmp/kernel-console.XXXXXX)
  printf '%s\r' "$1" >"$buf"
  chmod 644 "$buf"
  SCREENDIR="$SCREEN_DIR" runuser -u "$SCREEN_USER" -- screen -S "$SCREEN_SESSION" -p 0 -X readbuf "$buf"
  SCREENDIR="$SCREEN_DIR" runuser -u "$SCREEN_USER" -- screen -S "$SCREEN_SESSION" -p 0 -X paste .
  # screen reads the file asynchronously; give it a moment before deleting it.
  sleep 0.3
  rm -f -- "$buf"
}

server_port() {
  local port
  port=$(sed -n 's/^server-port=//p' "$MC_DIR/server.properties" 2>/dev/null | tr -d '\r')
  echo "${port:-25565}"
}

# Server list ping on localhost. Prints the status JSON, fails if the server doesn't answer.
server_ping() {
  python3 - "$(server_port)" <<'PY'
import socket, struct, sys

def varint(n):
    out = b""
    while True:
        b, n = n & 0x7F, n >> 7
        out += bytes([b | (0x80 if n else 0)])
        if not n:
            return out

def read_varint(s):
    n = shift = 0
    while True:
        b = s.recv(1)
        if not b:
            raise EOFError("connection closed")
        n |= (b[0] & 0x7F) << shift
        shift += 7
        if not b[0] & 0x80:
            return n

def packet(data):
    return varint(len(data)) + data

host, port = "127.0.0.1", int(sys.argv[1])
with socket.create_connection((host, port), timeout=5) as s:
    s.settimeout(5)
    addr = host.encode()
    s.sendall(packet(b"\x00" + varint(767) + varint(len(addr)) + addr + struct.pack(">H", port) + varint(1)))
    s.sendall(packet(b"\x00"))
    read_varint(s)  # packet length
    read_varint(s)  # packet id
    size = read_varint(s)
    data = b""
    while len(data) < size:
        chunk = s.recv(size - len(data))
        if not chunk:
            raise EOFError("connection closed")
        data += chunk
print(data.decode("utf-8"))
PY
}
