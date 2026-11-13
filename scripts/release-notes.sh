#!/usr/bin/env bash
# Prints the page of a release: what CHANGELOG.md says of the version before its first heading, how to install
# the package, then the notes of the version. The pictures and the files of the repository the notes link to are
# written as full addresses, at the tag of the version.
#
#   scripts/release-notes.sh 1.0.0 >notes.md
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:?usage: scripts/release-notes.sh <version>}"
repo="${GITHUB_REPOSITORY:-SensityTech/OpenIDEA-CfxLUA}"
tag="v$version"
id="$(sed -n 's/^id *= *"\(.*\)"/\1/p' plugin.toml | head -1)"

notes="$(awk -v heading="## [$version]" '
  index($0, heading) == 1 { found = 1; next }
  found && (/^## \[/ || /^\[[^]]+\]: /) { exit }
  found { print }
' CHANGELOG.md)"
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
  echo "release-notes: CHANGELOG.md has no notes for $version" >&2
  exit 1
fi

{
  printf '%s\n' "$notes" | awk '/^### / { exit } { print }'
  cat <<EOF

## Install

1. Download **\`$id-$version.oiplugin\`** below.
2. In OpenIDEA: **Settings › Plugins › Install Plugin from Package File…** and pick it.
3. Review the permissions and accept, then open a folder of resources.

Or install it from the marketplace of the project, signed by its publisher, with new versions offered as updates:
[how](README.md#from-the-openidea-cfxlua-marketplace). What the plugin needs is in the
[README](README.md#installation).

## Changes

EOF
  printf '%s\n' "$notes" | awk 'found || /^### / { found = 1; print }'
} |
  sed -E \
    -e "s#\]\((assets/[^)]+)\)#](https://raw.githubusercontent.com/$repo/$tag/\1)#g" \
    -e "s#\]\(([A-Za-z0-9_./-]+\.md)(\#[^)]*)?\)#](https://github.com/$repo/blob/$tag/\1\2)#g" |
  sed -e '/./,$!d' | cat -s
