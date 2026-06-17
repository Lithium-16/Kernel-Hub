# Cold War RTS (working title)

A large-scale, low-micro **logistics-focused real-time strategy game** — hundreds of
units per side, where you win by commanding **policy, logistics, and infrastructure**
rather than micromanaging units.

- **Design & decisions:** [PROJECT.md](PROJECT.md)
- **Build plan (phased):** [PLAN.md](PLAN.md)

> Status: **Milestone 0** — engine scaffold. No gameplay yet.

## Tech

Rust · [macroquad](https://github.com/not-fl3/macroquad) (render/input/window) ·
[hecs](https://github.com/Ralith/hecs) (ECS). Builds to **native** (performance) and
**WebAssembly** (browser).

## Build & run

### Native

```sh
cargo run
```

### WebAssembly

```sh
rustup target add wasm32-unknown-unknown
./scripts/build-wasm.sh
cd web && python -m http.server 8080
# open http://localhost:8080
```

## Development workflow

- Branch per phase/feature; merge to `main` via PR with green CI.
- [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, …).
- CI (GitHub Actions) runs build + test + clippy + wasm build on every push/PR.
