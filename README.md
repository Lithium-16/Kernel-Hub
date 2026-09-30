# Kernel

A Windows control panel for everything I run: the Minecraft server, my PCs, local AI
tools on Pluto, and files. Everything is built as modules, and there's an assistant
that can operate all of them. Every assistant action is also a button.

- **[ARCHITECTURE.md](ARCHITECTURE.md):** what it is and how the pieces fit
- **[PLAN.md](PLAN.md):** specs, data model, module specs, testing, and the phased build plan

Status: phase 0 (test run) is in progress. The node daemon, the protocol, the Python module
SDK, the Minecraft module, the desktop app (Tauri) and node self-updates work. Setting up
Pluto: **[docs/pluto-setup.md](docs/pluto-setup.md)**. Pairing, the tray and the assistant are next.

## What's here

| Path | What it is |
|---|---|
| `packages/protocol` | zod schemas for the node protocol (source of truth), JSON Schema output, fixtures |
| `crates/protocol` | Rust types for the same protocol, checked against the fixtures |
| `crates/node` | `kerneld`, the node daemon: runs modules, serves the WebSocket API, keeps the activity log, updates itself |
| `scripts/install-node.ps1` | installs a node on a Windows PC (Pluto) from the newest release |
| `sdk/python` | `kernel_sdk`, for writing modules in Python (MCP over stdio) |
| `modules/hello` | example module used by the tests |
| `modules/minecraft` | the Minecraft server on Pluto (WSL): start/stop, console, players, whitelist, backups |
| `modules/pc-monitor` | CPU, memory, disks, GPU, network; sleep/restart/shut down; Wake-on-LAN |
| `modules/roblox` | watches the Roblox AFK client: in game or disconnected, session time; rejoin/relaunch/close |
| `apps/desktop` | the desktop app: Tauri 2 shell + React. Screens are built from each module's actions and status |

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
uv run --python .venv --no-project pytest            # SDK and module tests
pnpm gen:schema   # after changing the zod schemas; commit the result
```

The node end-to-end test (`crates/node/tests/e2e.rs`) only runs when `KERNEL_TEST_PYTHON` is set.

### Running the desktop app

```sh
pnpm --filter @kernel/desktop dev          # in a browser at http://localhost:1420 (no window controls)
pnpm --filter @kernel/desktop tauri dev    # the real window, with Alt+Space for the palette
pnpm --filter @kernel/desktop tauri build  # Windows installer in apps/desktop/src-tauri/target/release/bundle/nsis
```

On first run it opens Settings: enter the node's address (`ws://pluto:47800/ws`) and its token.
Every CI run also uploads the Windows installer as the `kernel-desktop-windows` artifact.
Tauri on Linux needs the WebKitGTK dev packages (`libwebkit2gtk-4.1-dev` and friends).

### Running a node locally

Copy `crates/node/node.example.toml` somewhere private, set a real token, and point
`modules_dir` at `modules/` and `python` at the venv:

```sh
cargo run -p kernel-node -- --config path/to/node.toml
```

It listens on `127.0.0.1:47800` by default. Clients connect to `ws://<host>:47800/ws` and
send `hello` with the token first (see `packages/protocol/fixtures` for every message).
Logs go to `<data_dir>/logs/`, and each module's stderr goes to `<data_dir>/logs/modules/<id>.log`.
