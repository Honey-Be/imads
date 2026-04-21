#!/usr/bin/env bash
set -euo pipefail

# Build WASM Component Model component and generate JS bindings via jco.
# Usage: ./build-npm.sh [--release]
#
# Prerequisites:
#   cargo install cargo-component
#   npm install -g @bytecodealliance/jco @bytecodealliance/componentize-js

cd "$(dirname "$0")"

PROFILE="${1:---release}"

echo "==> Building WASM component"
cargo component build $PROFILE

# Determine the wasm binary path based on profile
if [ "$PROFILE" = "--release" ]; then
  WASM_PATH="target/wasm32-wasip2/release/imads_wasm.wasm"
else
  WASM_PATH="target/wasm32-wasip2/debug/imads_wasm.wasm"
fi

echo "==> Transpiling component to JS via jco"
jco transpile "$WASM_PATH" -o npm/bundler --name imads_wasm
jco transpile "$WASM_PATH" -o npm/nodejs --name imads_wasm --map 'honey-be:imads/*'
jco transpile "$WASM_PATH" -o npm/web --name imads_wasm

echo "==> Done. Package ready at imads-wasm/npm/"
echo "    npm pack (or npm link) from imads-wasm/npm/"
