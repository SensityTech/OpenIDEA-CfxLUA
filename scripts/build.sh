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
  if ! command -v zip >/dev/null; then
    echo "error: --pack needs zip (on Windows: scripts\\build.ps1 -Pack)" >&2
    exit 1
  fi
  id="$(sed -n 's/^id *= *"\(.*\)"/\1/p' plugin.toml | head -1)"
  version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' plugin.toml | head -1)"
  out="dist/$id-$version.oiplugin"
  stage="$(mktemp -d)"
  trap 'rm -rf "$stage" "$out.part"' EXIT
  mkdir -p "$stage/dist"
  cp plugin.toml settings.schema.json LICENSE NOTICE "$stage/"
  cp dist/plugin.wasm "$stage/dist/"
  for d in icons templates web; do
    if [ -d "$d" ]; then cp -r "$d" "$stage/"; fi
  done
  (cd "$stage" && zip -qr - .) >"$out.part"
  mv -f "$out.part" "$out"
  echo "$out"
fi
