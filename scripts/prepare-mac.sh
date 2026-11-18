#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl --fail --location --retry 3 --silent --show-error \
  https://registry.npmjs.org/@openai/codex/-/codex-0.170.0-darwin-arm64.tgz -o "$tmp/codex.tgz"
expected='sQyOGJabAUTEWCKe1O6GNsE7NKnHCHSjzLrazfYUgw8OFsFDeAQGVEwzvpXTK1yW1hJiLLAMRzC/W2Vu7m9DwQ=='
[[ "$(openssl dgst -sha512 -binary "$tmp/codex.tgz" | openssl base64 -A)" == "$expected" ]]
binary=package/vendor/aarch64-apple-darwin/bin/codex
tar -xzf "$tmp/codex.tgz" -C "$tmp" "$binary"
mkdir -p src-tauri/binaries
install -m 755 "$tmp/$binary" src-tauri/binaries/kladde-codex-aarch64-apple-darwin
