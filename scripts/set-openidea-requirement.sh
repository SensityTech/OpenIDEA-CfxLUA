#!/usr/bin/env bash
# Writes the OpenIDEA the plugin needs into the README, CONTRIBUTING and the user guide.
#
#   scripts/set-openidea-requirement.sh source   # no release has the plugin API yet: OpenIDEA built from the SDK revision
#   scripts/set-openidea-requirement.sh 0.1.36   # the first OpenIDEA release that installs the plugin
set -euo pipefail
cd "$(dirname "$0")/.."

target="${1:?usage: scripts/set-openidea-requirement.sh source|<version>}"
repo=https://github.com/l0ng-ai/openidea

if [ "$target" = source ]; then
  rev="$(sed -n 's|^source = "git+https://github.com/l0ng-ai/openidea?rev=\([0-9a-f]*\)#.*|\1|p' Cargo.lock | head -1)"
  [ -n "$rev" ] || { echo "set-openidea-requirement: no OpenIDEA SDK revision in Cargo.lock" >&2; exit 1; }
  short="${rev:0:7}"
  readme="| [OpenIDEA]($repo) built from source at [\`$short\`]($repo/commit/$rev) or later | The IDE with plugin API 1.0, which no OpenIDEA release has yet. Clone it at that revision or later, then \`pnpm install\` and \`pnpm tauri build\` (Rust, Node 22.18+, pnpm). |"
  contributing="- [OpenIDEA]($repo) built from source at \`$short\` or later (plugin API 1.0, in no release yet) to try the plugin."
  guide="- OpenIDEA built from source at revision \`$short\` or later: the plugin needs plugin API 1.0, which no OpenIDEA release has yet;"
else
  [[ "$target" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "set-openidea-requirement: $target is not a version" >&2; exit 1; }
  readme="| [OpenIDEA]($repo/releases/latest) $target or newer | The IDE (plugin API 1.0). |"
  contributing="- [OpenIDEA]($repo) $target or newer to try the plugin."
  guide="- OpenIDEA $target or newer;"
fi

replace() {
  local file="$1" prefix="$2" line="$3" n
  n="$(PREFIX="$prefix" awk 'index($0, ENVIRON["PREFIX"]) == 1 { n++ } END { print n + 0 }' "$file")"
  [ "$n" = 1 ] || { echo "set-openidea-requirement: $file has $n lines starting with '$prefix'" >&2; exit 1; }
  PREFIX="$prefix" LINE="$line" awk 'index($0, ENVIRON["PREFIX"]) == 1 { print ENVIRON["LINE"]; next } { print }' \
    "$file" >"$file.tmp"
  mv "$file.tmp" "$file"
}

replace README.md "| [OpenIDEA](" "$readme"
replace CONTRIBUTING.md "- [OpenIDEA](" "$contributing"
replace data/docs/guides/plugin-guide.md "- OpenIDEA " "$guide"
