#!/usr/bin/env bash
# TMPDIR on a RAM disk for one command (openspec AQ.35): makes a 2 GiB APFS RAM
# disk, runs the command with TMPDIR on it, detaches the disk after, an
# interrupted run (Ctrl-C, a hangup, a TERM) included, and exits with the
# command's status. guac-extract's `write_file` ends every save with
# `sync_all` (`F_FULLFSYNC` on macOS, about 4 ms), and every test writing
# through it into a temp dir pays that on the SSD; here every flush is still
# called and answered at once (AQ.35: `cargo test --workspace` 34.5-39.6 s on
# the SSD, 16.6-19.8 s through this script, its 2-3 s of making and detaching
# included).
#
# APFS only: an HFS+ disk's whole-second times fail guac-extract's
# `identity.rs` every run, so a volume that mounts as anything else is
# detached and refused. A disk that can't be made or is refused doesn't block
# the run: the command runs on the usual TMPDIR, slower, with a line saying
# so. Each run gets its own volume, /Volumes/guac-tmp-<pid> (two lanes pushing
# at once take 4 GiB), TMPDIR its `tmp/` folder, a `.metadata_never_index` at
# its root for a Mac with Spotlight on (this one has it off, and the detach
# measured the same either way: 0.3-0.5 s). A run killed outright
# (SIGKILL) can't detach; the next run detaches any guac-tmp-<pid> RAM disk
# whose process is gone. `hdiutil info` lists what's attached. Mac only:
# anywhere else the command runs as it is.
#
#   .githooks/ram-tmpdir.sh <command> [args...]
#
# The whole script is one brace group, read before any of it runs: bash reads
# a script as it goes, so an edit or a branch switch landing mid-run would
# otherwise feed it a half line (AQ.35: an edit to this file during an
# 11-minute run ended it in a syntax error).
{
set -uo pipefail

if [ $# -eq 0 ]; then
  echo "usage: .githooks/ram-tmpdir.sh <command> [args...]" >&2
  exit 2
fi
[ "$(uname -s)" = Darwin ] || exec "$@"

prefix=guac-tmp
name=$prefix-$$
mnt=/Volumes/$name
sectors=$((2 * 1024 * 1024 * 2)) # 2 GiB of 512-byte sectors

say() { printf 'ram-tmpdir: %s\n' "$*" >&2; }
# Runs a command in a process group of its own, so a Ctrl-C at the terminal
# doesn't reach it: diskutil catches SIGINT itself (an ignored one too), and
# an attach it abandons can leave a RAM device attached that no one named.
# The script's INT trap runs once the command returns.
solo() { perl -e 'setpgrp(0, 0); exec @ARGV or die "exec $ARGV[0]: $!\n"' "$@" </dev/null; }
now() { perl -MTime::HiRes=time -e 'printf "%.3f", time'; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.1f", b - a }'; }

# The RAM device behind a mounted volume: its APFS container's one physical
# store, accepted only when `hdiutil info` lists that device as a ram:// image.
ram_store() {
  local store
  store=$(diskutil info -plist "$1" 2>/dev/null |
    plutil -extract APFSPhysicalStores.0.APFSPhysicalStore raw - 2>/dev/null) || return 1
  store=/dev/${store%s[0-9]*}
  hdiutil info | awk -v d="$store" '
    /^image-path/ { ram = ($3 ~ /^ram:\/\//) }
    $1 == d && ram { found = 1 }
    END { exit !found }' && echo "$store"
}

# Eject a RAM device; force it when a process still holds a file on it (a
# test's child an interrupted run left behind).
eject() {
  local i
  for i in 1 2 3; do
    solo diskutil eject "$1" >/dev/null 2>&1 && return 0
    sleep 1
  done
  solo diskutil unmountDisk force "$1" >/dev/null 2>&1
  solo diskutil eject "$1" >/dev/null 2>&1 && return 0
  say "could not detach $1: run 'diskutil unmountDisk force $1; diskutil eject $1'"
  return 1
}

# A run killed outright left its volume: detach every guac-tmp-<pid> RAM disk
# whose process is gone (or is this one, a reused pid's).
for v in /Volumes/$prefix-*; do
  [ -d "$v" ] || continue
  pid=${v##*-}
  case $pid in '' | *[!0-9]*) continue ;; esac
  if [ "$pid" = $$ ] || ! ps -p "$pid" >/dev/null 2>&1; then
    if store=$(ram_store "$v"); then
      say "detaching $v ($store), left by a run that's gone"
      eject "$store"
    fi
  fi
done

dev=
devfile=$(mktemp "${TMPDIR:-/tmp}/ram-tmpdir.XXXXXX") || exit 1
cleanup() {
  local rc=$? t used
  trap - EXIT
  trap '' INT TERM HUP
  [ -n "$dev" ] || dev=$(awk 'NR == 1 { print $1 }' "$devfile" 2>/dev/null)
  rm -f "$devfile"
  if [ -n "$dev" ]; then
    used=$(df -k "$mnt" 2>/dev/null | awk 'NR == 2 { printf "%.0f", $3 / 1024 }')
    t=$(now)
    if eject "$dev"; then
      say "detached $dev in $(since "$t") s${used:+, $used MiB used at the end}"
    else
      rc=1
    fi
  fi
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

# Makes the disk, TMPDIR's folder on it; a step that fails says why and
# leaves anything attached in $dev for the caller to eject. Attaching and
# formatting run solo, so a Ctrl-C can't land between attaching the device
# and noting it.
make_disk() {
  local out fs
  if ! solo diskutil image attach --noMount "ram://$sectors" >"$devfile" 2>&1; then
    say "could not attach a RAM disk: $(tr '\n' ' ' <"$devfile")"
    : >"$devfile"
    return 1
  fi
  dev=$(awk 'NR == 1 { print $1 }' "$devfile")
  case $dev in
  /dev/disk[0-9]*) ;;
  *)
    say "unexpected attach output: $(tr '\n' ' ' <"$devfile")"
    dev=
    : >"$devfile"
    return 1
    ;;
  esac
  if ! out=$(solo diskutil erasevolume APFS "$name" "$dev" 2>&1); then
    say "could not format $dev as APFS: $(echo "$out" | tr '\n' ' ')"
    return 1
  fi
  fs=$(diskutil info -plist "$mnt" 2>/dev/null | plutil -extract FilesystemType raw - 2>/dev/null)
  if [ "$fs" != apfs ]; then
    say "refused: $mnt is ${fs:-not mounted}, not APFS (an HFS+ disk's whole-second times fail guac-extract's identity.rs)"
    return 1
  fi
  if [ "$(ram_store "$mnt")" != "$dev" ]; then
    say "refused: $mnt isn't the RAM disk just made ($dev)"
    return 1
  fi
  touch "$mnt/.metadata_never_index" && mkdir "$mnt/tmp"
}

t=$(now)
if make_disk; then
  say "TMPDIR=$mnt/tmp (a 2 GiB APFS RAM disk, $dev, made in $(since "$t") s)"
  TMPDIR=$mnt/tmp "$@"
else
  # The tests pass on the SSD too, just slower: a disk that can't be made
  # doesn't block the run.
  if [ -n "$dev" ] && eject "$dev"; then
    dev=
    : >"$devfile"
  fi
  say "running on ${TMPDIR:-the default temp dir} instead"
  "$@"
fi
exit
}
