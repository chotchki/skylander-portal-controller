#!/usr/bin/env bash
# Apply the oracle series (O1 up, openspec AF) onto an RPCS3 checkout that already
# carries the product's P1-P8, the way rpcs3-patches/apply.sh leaves it. Dev-only:
# the oracle is Guacamole's fidelity oracle, never a product build.
#
#   rpcs3-patches/oracle/apply.sh <rpcs3-checkout>          # apply
#   rpcs3-patches/oracle/apply.sh --check <rpcs3-checkout>  # exit 0 iff the whole stack is on HEAD
#
# The checkout must sit OUTSIDE vendor/rpcs3 (AF design decision 1): the submodule stays
# on its pin, and its build dir stays the product's. HEAD must be P8, with P1-P8 the
# eight commits under it, matched by subject against rpcs3-patches/0*.patch. Upstream
# files the O-series touches can be stored CRLF (VKRenderPass.h is), and `git am`
# strips CR from a patch, so those files are normalized to LF in one commit under O1,
# the product series' own trick (rpcs3-patches/README.md, "The CRLF base commit"); a
# re-export runs `git format-patch` from that commit, HEAD~<#O-patches>.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
product="$here/.."
check=0
if [[ "${1:-}" == "--check" ]]; then
  check=1
  shift
fi
target="${1:?usage: rpcs3-patches/oracle/apply.sh [--check] <rpcs3-checkout>}"

if [[ ! -e "$target/.git" ]]; then
  echo "error: '$target' is not a git checkout." >&2
  exit 1
fi
target="$(cd "$target" && pwd -P)"
vendor="$(cd "$product/../vendor" 2>/dev/null && pwd -P)/rpcs3"
if [[ "$target" == "$vendor" ]]; then
  echo "error: the oracle never builds in vendor/rpcs3 (the product's pinned submodule)." >&2
  echo "       Use a checkout outside the repo; rpcs3-patches/oracle/build.sh makes one." >&2
  exit 1
fi

# The subject `git am` gives a patch file's commit: mailinfo unfolds the header and
# drops the [PATCH n/m] prefix exactly as `git am` does.
subject_of() {
  local scratch
  scratch="$(mktemp -d)"
  git mailinfo "$scratch/msg" "$scratch/patch" <"$1" | sed -n 's/^Subject: //p'
  rm -rf "$scratch"
}

# Subjects of the commits HEAD~(skip+count-1)..HEAD~skip, oldest first.
subjects_at() {
  git -C "$target" log --reverse --format=%s --skip="$1" -n "$2" HEAD
}

# 0 iff the patch files' subjects are, in order, the commits `skip` back from HEAD.
stack_matches() {
  local skip="$1"
  shift
  local want got
  want="$(for p in "$@"; do subject_of "$p"; done)"
  got="$(subjects_at "$skip" "$#")"
  [[ "$want" == "$got" ]]
}

p_patches=("$product"/0*.patch)
o_patches=("$here"/0*.patch)
np=${#p_patches[@]}
no=${#o_patches[@]}
norm_subject="normalize CRLF->LF on oracle-series-touched files (pre-am base)"

if (( check )); then
  # O on top, then the optional normalization commit, then P1-P8.
  stack_matches 0 "${o_patches[@]}" || exit 1
  skip=$no
  if [[ "$(subjects_at "$skip" 1)" == "$norm_subject" ]]; then
    skip=$((skip + 1))
  fi
  stack_matches "$skip" "${p_patches[@]}" || exit 1
  skip=$((skip + np))
  # rpcs3-patches/apply.sh's own normalization commit, when it made one, sits under P1.
  if [[ "$(subjects_at "$skip" 1)" == "normalize CRLF->LF on series-touched files (pre-am base)" ]]; then
    skip=$((skip + 1))
  fi
  echo "oracle series on HEAD: $np P + $no O over $(git -C "$target" rev-parse --short "HEAD~$skip")"
  exit 0
fi

if ! stack_matches 0 "${p_patches[@]}"; then
  echo "error: HEAD of '$target' isn't P8 with P1-P8 under it (by subject)." >&2
  echo "       Apply the product series first: rpcs3-patches/apply.sh '$target'" >&2
  echo "       Top $np commits:" >&2
  subjects_at 0 "$np" | sed 's/^/         /' >&2
  exit 1
fi

cd "$target"
git config user.email >/dev/null 2>&1 || git config user.email "patch-bot@skylander-portal-controller.local"
git config user.name  >/dev/null 2>&1 || git config user.name  "skylander patch-bot"
base="$(git rev-parse --short HEAD)"

# LF on both sides, as rpcs3-patches/apply.sh does for the P-series: the patch files,
# then the existing files they touch (auto-detected from `+++ b/<file>`). Only those
# files are staged, never `commit -a` (a dirty 3rdparty gitlink would ride along).
perl -i -pe 's/\r\n/\n/g' "${o_patches[@]}"
touched=()
for f in $(grep -h '^+++ b/' "${o_patches[@]}" | sed -e 's#^+++ b/##' -e 's/\r$//' | sort -u); do
  [ -f "$f" ] || continue
  perl -i -pe 's/\r\n/\n/g' "$f"
  touched+=("$f")
done
if (( ${#touched[@]} )); then
  git add -- "${touched[@]}"
  if git diff --cached --quiet -- "${touched[@]}"; then
    echo "oracle/apply.sh: oracle-series-touched source already LF, no normalization commit."
  else
    echo "oracle/apply.sh: normalized CRLF->LF on:"
    git diff --cached --name-only -- "${touched[@]}" | sed 's/^/    /'
    git commit -qm "$norm_subject" -- "${touched[@]}"
  fi
fi

echo "Applying $no oracle patch(es) onto $base in $target ..."
git am --3way "${o_patches[@]}"
echo "OK, $np P + $no O applied; HEAD now $(git rev-parse --short HEAD) (was $base)"
