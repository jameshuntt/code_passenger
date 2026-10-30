#!/bin/sh
# Puts each crate siblings.toml names at ../<name>, at its commit, so this checkout builds against exactly what its
# tests were run against (Cargo.toml names the crate by path; Cargo.lock cannot pin a path dependency).
#
# A sibling already at ../<name> must be at that commit: it is checked and never changed. A missing one is cloned and
# its commit checked out. SIBLING_<NAME>_REPOSITORY (the name in capitals) takes the repository from elsewhere, a
# local mirror for one: the commit is still the one pinned, and is checked.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
parent=$(dirname "$here")
rows=$(awk -F'"' '
  /^\[\[sibling\]\]/ { if (n != "") print n "\t" r "\t" c; n = ""; r = ""; c = "" }
  /^name *=/ { n = $2 }
  /^repository *=/ { r = $2 }
  /^commit *=/ { c = $2 }
  END { if (n != "") print n "\t" r "\t" c }
' "$here/siblings.toml")
[ -n "$rows" ] || { echo "siblings.toml names no sibling" >&2; exit 1; }
status=0
tab=$(printf '\t')
echo "$rows" | while IFS="$tab" read -r name repo commit; do
  dest="$parent/$name"
  var="SIBLING_$(echo "$name" | tr '[:lower:]-' '[:upper:]_')_REPOSITORY"
  override=$(eval "printf '%s' \"\${$var:-}\"")
  [ -n "$override" ] && repo=$override
  if [ -d "$dest" ]; then
    at=$(git -C "$dest" rev-parse HEAD 2>/dev/null || echo none)
    if [ "$at" != "$commit" ]; then
      echo "$name: $dest is at $at, siblings.toml pins $commit; check it out there (this script never changes an existing checkout)" >&2
      exit 1
    fi
    if [ -n "$(git -C "$dest" status --porcelain --untracked-files=no)" ]; then
      echo "$name: $dest is at $commit with uncommitted changes; the build would not be the pinned one" >&2
      exit 1
    fi
    echo "$name: $dest at $commit (already there)"
  else
    git clone --quiet "$repo" "$dest"
    git -C "$dest" checkout --quiet --detach "$commit"
    echo "$name: $dest at $(git -C "$dest" rev-parse HEAD) (cloned from $repo)"
  fi
done || status=$?
exit $status
