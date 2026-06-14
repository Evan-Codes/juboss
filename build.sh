#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

export PATH="${HOME}/.cargo/bin:${PATH}"

mode="${1:-debug}"

echo "==> Svelte type check"
npm run check

echo "==> Frontend build"
npm run build

echo "==> Rust check"
(cd src-tauri && cargo check)

case "${mode}" in
  debug)
    echo "==> Tauri debug app bundle"
    npm run tauri -- build --debug --bundles app
    echo "==> Built: src-tauri/target/debug/bundle/macos/Juboss Desktop Pet.app"
    ;;
  release)
    echo "==> Tauri release app bundle"
    npm run tauri -- build --bundles app
    echo "==> Built: src-tauri/target/release/bundle/macos/Juboss Desktop Pet.app"
    ;;
  *)
    echo "Usage: ./build.sh [debug|release]" >&2
    exit 2
    ;;
esac
