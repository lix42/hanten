#!/usr/bin/env bash
# Build the frozen reference binary and cache it outside the repo.
#
#   scripts/reference-snapshot/build.sh [ref]      # ref defaults to `reserve`
#
# Prints the cached binary's path on stdout (everything else goes to stderr), so
# `NC_REF=$(scripts/reference-snapshot/build.sh)` works. See README.md beside it.
#
# Environment:
#   HANTEN_REFERENCE_CACHE   cache root (default: ~/Library/Caches/hanten-reference
#                            on macOS, ${XDG_CACHE_HOME:-~/.cache}/hanten-reference
#                            elsewhere)
#   HANTEN_REFERENCE_TARGET  cargo target dir to build in (default: a fresh
#                            temporary one, removed afterwards)
#   HANTEN_REFERENCE_REBUILD=1  build even when the cache already has this commit
set -euo pipefail

ref=${1:-reserve}
tag=pre-new-flow   # where `reserve` started; only used for the "has it moved" note

say() { printf '%s\n' "$*" >&2; }
die() { say "reference-snapshot: error: $*"; exit 1; }

repo=$(git rev-parse --show-toplevel) || die "run this from inside the hanten repository"

# Prefer the remote's view of the ref: `reserve` is shared, and a stale local branch
# would silently build an older reference. When the fetch fails, fall back to the
# local ref for real — the remote-tracking ref is then the stale one.
fetched=0
if git -C "$repo" remote get-url origin >/dev/null 2>&1; then
  if git -C "$repo" fetch --quiet origin "+refs/heads/$ref:refs/remotes/origin/$ref" 2>/dev/null; then
    fetched=1
  elif git -C "$repo" fetch --quiet origin "refs/tags/$ref:refs/tags/$ref" 2>/dev/null; then
    fetched=1
  else
    say "reference-snapshot: note: could not fetch '$ref' from origin; using the local ref"
  fi
fi
remote_commit=$(git -C "$repo" rev-parse --verify --quiet "refs/remotes/origin/$ref^{commit}" || true)
local_commit=$(git -C "$repo" rev-parse --verify --quiet "$ref^{commit}" || true)
if [[ $fetched == 1 && -n $remote_commit ]]; then
  commit=$remote_commit
else
  commit=${local_commit:-$remote_commit}
fi
[[ -n $commit ]] || die "cannot resolve '$ref' to a commit"

if [[ -z ${HANTEN_REFERENCE_CACHE:-} ]]; then
  if [[ $(uname -s) == Darwin ]]; then
    HANTEN_REFERENCE_CACHE=$HOME/Library/Caches/hanten-reference
  else
    HANTEN_REFERENCE_CACHE=${XDG_CACHE_HOME:-$HOME/.cache}/hanten-reference
  fi
fi
# Keyed by the resolved commit, never by the ref name alone: when `reserve` moves,
# the new build lands beside the old one instead of replacing it. The key carries no
# host or toolchain, so the cache is per machine — BUILD_INFO records both.
dest=$HANTEN_REFERENCE_CACHE/$ref/$commit

# The binary must report exactly the commit it is filed under, from a clean tree.
# `--version` prints `commit: <12-hex>` — suffixed `-dirty` or `(dirty unknown)`
# otherwise — so an exact match on that line rejects all three. This is the check
# that keeps a mislabelled binary out of the cache.
verify() {
  local bin=$1 hint=${2:-} banner
  banner=$("$bin" --version) || die "$bin --version failed$hint"
  grep -qx "commit: ${commit:0:12}" <<<"$banner" \
    || die "$bin does not report a clean build of ${commit:0:12}:"$'\n'"$banner$hint"
}

sha256() {
  if command -v shasum >/dev/null; then shasum -a 256 "$1"; else sha256sum "$1"; fi | cut -d' ' -f1
}

if [[ -x $dest/nc && ${HANTEN_REFERENCE_REBUILD:-0} != 1 ]]; then
  verify "$dest/nc" $'\n'"(the cached binary is bad; rerun with HANTEN_REFERENCE_REBUILD=1)"
  say "reference-snapshot: cached $ref @ ${commit:0:12}"
else
  work=$(mktemp -d "${TMPDIR:-/tmp}/hanten-reference.XXXXXX")
  staging=""
  cleanup() {
    git -C "$repo" worktree remove --force "$work/src" >/dev/null 2>&1 || true
    rm -rf "$work"
    [[ -z $staging ]] || rm -rf "$staging"
  }
  trap cleanup EXIT

  say "reference-snapshot: building $ref @ ${commit:0:12}"
  git -C "$repo" worktree add --quiet --detach "$work/src" "$commit"
  target=${HANTEN_REFERENCE_TARGET:-$work/target}
  # Absolute before the build `cd`s into the worktree, or cargo and the lookup
  # below would resolve a relative override against different directories.
  [[ $target == /* ]] || target=$PWD/$target
  # Built from inside the worktree, so cargo config and any rust-toolchain file are
  # the reference commit's own, not those of the checkout running this script.
  (cd "$work/src" && CARGO_TARGET_DIR=$target cargo build --release --locked >&2)

  # The binary was renamed `nc` -> `hanten` on 2026-09-21, after `reserve` started;
  # accept either so a later cherry-pick of the rename cannot break the script. In a
  # reused target dir both may exist, and the one just built is the newer.
  built=""
  for name in nc hanten; do
    candidate=$target/release/$name
    [[ -x $candidate ]] || continue
    if [[ -z $built || $candidate -nt $built ]]; then built=$candidate; fi
  done
  [[ -n $built ]] || die "no nc/hanten binary in $target/release"
  verify "$built"

  # Staged, then moved into place, so a half-copied binary never looks cached. Not
  # safe against two runs installing the same commit at once.
  mkdir -p "$(dirname "$dest")"
  staging=$(mktemp -d "$(dirname "$dest")/.staging.XXXXXX")
  cp "$built" "$staging/nc"
  {
    echo "ref: $ref"
    echo "commit: $commit"
    echo "binary: $(basename "$built")"
    echo "version: $("$built" --version | head -1)"
    echo "rustc: $(cd "$work/src" && rustc -V)"
    echo "host: $(cd "$work/src" && rustc -vV | sed -n 's/^host: //p')"
    echo "sha256: $(sha256 "$staging/nc")"
    echo "built: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  } >"$staging/BUILD_INFO"
  rm -rf "$dest"
  mv "$staging" "$dest"
  staging=""
  say "reference-snapshot: cached at $dest"
fi

# Only `reserve` starts at the tag, so only it can have moved past it.
if [[ $ref == reserve ]]; then
  tag_commit=$(git -C "$repo" rev-parse --verify --quiet "refs/tags/$tag^{commit}" || true)
  if [[ -n $tag_commit && $tag_commit != "$commit" ]]; then
    say "reference-snapshot: note: $ref has moved past $tag (${tag_commit:0:12});" \
        "renders from this binary are labelled ${commit:0:12}, not the tag"
  fi
fi

printf '%s\n' "$dest/nc"
