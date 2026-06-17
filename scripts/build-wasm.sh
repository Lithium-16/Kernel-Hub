#!/usr/bin/env bash
# Build the WASM target and stage it next to web/index.html.
set -euo pipefail

cargo build --release --target wasm32-unknown-unknown
mkdir -p web
cp "target/wasm32-unknown-unknown/release/coldwar-rts.wasm" "web/coldwar-rts.wasm"

echo "WASM staged at web/coldwar-rts.wasm"
echo "Serve it:  (cd web && python -m http.server 8080)  then open http://localhost:8080"
