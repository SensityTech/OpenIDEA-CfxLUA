#!/usr/bin/env bash
# Builds the plugin component (dist/plugin.wasm). With --pack, also the
# installable package dist/<id>-<version>.oiplugin.
#
#   scripts/build.sh            # build
#   scripts/build.sh --pack     # build + package
set -euo pipefail
cd "$(dirname "$0")/.."

rustup target list --installed | grep -qx wasm32-wasip2 || rustup target add wasm32-wasip2
cargo build --release --locked --target wasm32-wasip2
mkdir -p dist
cp target/wasm32-wasip2/release/openidea_cfxlua.wasm dist/plugin.wasm
echo "dist/plugin.wasm ($(wc -c <dist/plugin.wasm | tr -d ' ') bytes)"

if [ "${1:-}" = "--pack" ]; then
  id="$(sed -n 's/^id *= *"\(.*\)"/\1/p' plugin.toml | head -1)"
  version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' plugin.toml | head -1)"
  out="dist/$id-$version.oiplugin"
  stage="$(mktemp -d)"
  trap 'rm -rf "$stage"' EXIT
  mkdir -p "$stage/dist"
  cp plugin.toml settings.schema.json LICENSE NOTICE "$stage/"
  cp dist/plugin.wasm "$stage/dist/"
  cp -r icons templates web "$stage/"
  rm -f "$out"
  (cd "$stage" && zip -qr - .) >"$out"
  echo "$out"
fi
