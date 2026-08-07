#!/bin/bash
# TEMPORARY PoC: build the browser WASM (release) + wasm-bindgen glue + wasm-opt.
# Output: browser-poc/pkg/{nettest.js, nettest_bg.wasm}
set -e
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"

echo "==> cargo build --release --target wasm32-unknown-unknown"
cargo build --release --target wasm32-unknown-unknown --lib

echo "==> wasm-bindgen"
wasm-bindgen target/wasm32-unknown-unknown/release/nettest.wasm \
  --out-dir browser-poc/pkg --target web

if command -v wasm-opt >/dev/null 2>&1; then
  echo "==> wasm-opt -O3"
  wasm-opt -O3 browser-poc/pkg/nettest_bg.wasm -o browser-poc/pkg/nettest_bg.wasm
else
  echo "==> wasm-opt not found, skipping"
fi

ls -la browser-poc/pkg/nettest_bg.wasm | awk '{print "bg.wasm: "$5" bytes"}'
echo "Done. Serve browser-poc/ and open wasm.html"
