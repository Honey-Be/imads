#!/usr/bin/env bash
set -euo pipefail

# Build all three wasm-pack targets and assemble into npm/
# Usage: ./build-npm.sh [--release]

cd "$(dirname "$0")"

PROFILE="${1:---release}"

echo "==> Building --target bundler"
wasm-pack build --target bundler $PROFILE --out-dir npm/bundler --out-name imads_wasm
rm -f npm/bundler/package.json npm/bundler/.gitignore

echo "==> Building --target web"
wasm-pack build --target web $PROFILE --out-dir npm/web --out-name imads_wasm
rm -f npm/web/package.json npm/web/.gitignore

echo "==> Building --target nodejs"
wasm-pack build --target nodejs $PROFILE --out-dir npm/nodejs --out-name imads_wasm
rm -f npm/nodejs/package.json npm/nodejs/.gitignore

echo "==> Done. Package ready at imads-wasm/npm/"
echo "    npm pack (or npm link) from imads-wasm/npm/"
