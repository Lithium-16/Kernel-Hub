# Kernel

A Windows control panel for everything I run: the Minecraft server, my PCs, local AI
tools on Pluto, and files. Everything is built as modules, and there's an assistant
that can operate all of them. Every assistant action is also a button.

- **[ARCHITECTURE.md](ARCHITECTURE.md):** what it is and how the pieces fit
- **[PLAN.md](PLAN.md):** specs, data model, module specs, testing, and the phased build plan

Status: phase 0 (test run) is in progress. The node daemon, the protocol, the Python module
SDK and a `hello` module work. The desktop app, pairing and the assistant are next.

## What's here

| Path | What it is |
|---|---|
| `packages/protocol` | zod schemas for the node protocol (source of truth), JSON Schema output, fixtures |
| `crates/protocol` | Rust types for the same protocol, checked against the fixtures |
| `crates/node` | `kerneld`, the node daemon: runs modules, serves the WebSocket API, keeps the activity log |
| `sdk/python` | `kernel_sdk`, for writing modules in Python (MCP over stdio) |
| `modules/hello` | example module used by the tests |

## Development

You need Rust (the version is pinned in `rust-toolchain.toml`), Node 22 with pnpm, and [uv](https://docs.astral.sh/uv/).

```sh
pnpm install
uv venv .venv && uv pip install --python .venv -e "sdk/python[dev]"

# checks (the same ones CI runs)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
KERNEL_TEST_PYTHON=$PWD/.venv/bin/python cargo test --workspace   # Windows: .venv\Scripts\python.exe
pnpm lint && pnpm typecheck && pnpm test
uv run --python .venv --no-project pytest sdk/python
pnpm gen:schema   # after changing the zod schemas; commit the result
```

The node end-to-end test (`crates/node/tests/e2e.rs`) only runs when `KERNEL_TEST_PYTHON` is set.

### Running a node locally

Copy `crates/node/node.example.toml` somewhere private, set a real token, and point
`modules_dir` at `modules/` and `python` at the venv:

```sh
cargo run -p kernel-node -- --config path/to/node.toml
```

It listens on `127.0.0.1:47800` by default. Clients connect to `ws://<host>:47800/ws` and
send `hello` with the token first (see `packages/protocol/fixtures` for every message).
Logs go to `<data_dir>/logs/`, and each module's stderr goes to `<data_dir>/logs/modules/<id>.log`.
