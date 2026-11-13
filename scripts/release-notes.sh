#!/usr/bin/env bash
# Prints the notes of a version as CHANGELOG.md has them, for the page of its release: the pictures and the
# files of the repository they link to are written as full addresses, at the tag of the version.
#
#   scripts/release-notes.sh 1.0.0 >notes.md
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:?usage: scripts/release-notes.sh <version>}"
repo="${GITHUB_REPOSITORY:-SensityTech/OpenIDEA-CfxLUA}"
tag="v$version"

notes="$(awk -v heading="## [$version]" '
  index($0, heading) == 1 { found = 1; next }
  found && (/^## \[/ || /^\[[^]]+\]: /) { exit }
  found { print }
' CHANGELOG.md)"
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
  echo "release-notes: CHANGELOG.md has no notes for $version" >&2
  exit 1
fi

printf '%s\n' "$notes" |
  sed -E \
    -e "s#\]\((assets/[^)]+)\)#](https://raw.githubusercontent.com/$repo/$tag/\1)#g" \
    -e "s#\]\(([A-Za-z0-9_./-]+\.md)(\#[^)]*)?\)#](https://github.com/$repo/blob/$tag/\1\2)#g" |
  sed -e '/./,$!d'
