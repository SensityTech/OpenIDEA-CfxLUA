#!/usr/bin/env bash
# Signs the package of a release and publishes it, with the marketplace index, on the `marketplace` release.
# Needs gh, jq, unzip, openidea-plugin, and SIGNING_KEY: the content of the file `openidea-plugin keygen` writes.
#
#   SIGNING_KEY="$(cat sensity.key)" scripts/publish-marketplace.sh v0.1.1
set -euo pipefail
cd "$(dirname "$0")/.."

tag="${1:?usage: scripts/publish-marketplace.sh <tag>}"
: "${SIGNING_KEY:?SIGNING_KEY is not set}"
market=marketplace
publisher=sensity
publisher_name=SensityTech
repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
index_url="https://github.com/$repo/releases/download/$market/index.json"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
key="$work/publisher.key"
(umask 077 && printf '%s' "$SIGNING_KEY" >"$key")
mkdir -p "$work/release" "$work/stage" "$work/market"

gh release download "$tag" --pattern '*.oiplugin' --dir "$work/release"
set -- "$work/release"/*.oiplugin
[ "$#" -eq 1 ] || { echo "publish-marketplace: expected one package in $tag, found $#" >&2; exit 1; }
unzip -q "$1" -d "$work/stage"
id="$(sed -n 's/^id *= *"\(.*\)"/\1/p' "$work/stage/plugin.toml" | head -1)"
version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$work/stage/plugin.toml" | head -1)"
[ "v$version" = "$tag" ] || { echo "publish-marketplace: $tag holds version $version" >&2; exit 1; }
package="$id-$version.oiplugin"

openidea-plugin pack "$work/stage" --key "$key" --publisher "$publisher" --out "$work/market/$package"

if assets="$(gh release view "$market" --json assets --jq '.assets[].name' 2>/dev/null)"; then
  if printf '%s\n' "$assets" | grep -qx index.json; then
    gh release download "$market" --pattern index.json --dir "$work/market"
    listed="$(jq -r --arg id "$id" '.plugins[] | select(.id == $id) | .version' "$work/market/index.json")"
    if [ -n "$listed" ] && [ "$listed" != "$version" ] &&
      [ "$(printf '%s\n' "$listed" "$version" | sort -V | tail -1)" = "$listed" ]; then
      echo "publish-marketplace: the marketplace lists $id $listed, newer than $version" >&2
      exit 1
    fi
  fi
else
  gh release create "$market" --prerelease --title "Marketplace" --notes "Not a version: the signed packages and \
the index of the OpenIDEA-CfxLUA marketplace. In OpenIDEA, **Settings › Plugins › Available**, set the marketplace \
index URL to \`$index_url\`. Versions and their notes are in the other releases."
fi

openidea-plugin index "$work/market" --publisher "$publisher" --name "$publisher_name" --key "$key" \
  --out "$work/market/index.json"
gh release upload "$market" "$work/market/$package" "$work/market/index.json" --clobber
echo "published $id $version: $index_url"
