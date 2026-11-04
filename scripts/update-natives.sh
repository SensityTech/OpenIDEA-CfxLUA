#!/usr/bin/env sh
# Refreshes the bundled native databases in data/natives from their sources (src/natives/sources.rs).
# Files whose content did not change are left untouched, so `git diff` tells whether there is anything to commit.
set -eu
cd "$(dirname "$0")/.."

list=$(cargo test --lib --locked natives::sources::tests::database_list -- --ignored --exact --nocapture 2>/dev/null |
  sed -n 's/^natives-db //p')
if [ -z "$list" ]; then
  echo "update-natives: no database listed by natives::sources::tests::database_list" >&2
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "$list" | while read -r file url; do
  curl --fail --silent --show-error --location --retry 3 --max-time 120 "$url" -o "$tmp/$file"
  if [ "$(wc -c < "$tmp/$file")" -lt 100000 ]; then
    echo "update-natives: $url returned $(wc -c < "$tmp/$file") bytes, keeping the bundled file" >&2
    exit 1
  fi
  bundled="data/natives/$file.gz"
  if [ -f "$bundled" ] && gzip -dc "$bundled" | cmp -s - "$tmp/$file"; then
    echo "unchanged  $file"
  else
    gzip -9 -n -c "$tmp/$file" > "$bundled"
    echo "updated    $file"
  fi
done
