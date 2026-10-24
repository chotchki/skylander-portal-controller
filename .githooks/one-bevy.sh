#!/usr/bin/env bash
# One Bevy for guacamole's build and its tests (openspec AQ.30): fails when a
# dev-dependency turns on a feature of a crate the build shares with Bevy's
# graph, which would make `cargo test -p guacamole` build a second Bevy and
# Avian beside `cargo build -p guacamole`'s (AQ.3: 272.8 s and 1.17 GiB per cold
# target dir). Reads cargo tree's resolved features with no build: the build's
# graph (`-e normal,build`) against the tests' (`-e normal,build,dev`), which is
# how cargo resolves each.
#
# A third-party crate whose features differ is a failure when a third-party
# crate in both graphs depends on it: everything above it builds twice. One
# that only our crates or test-only crates depend on builds twice by itself
# (guac-formats' `test-fixtures` turning on lzma-rust2's encoder), so it's
# printed and passes. The fix is the line in crates/guacamole/Cargo.toml that
# names the crate with the tests' features (rustix's, today).
#
#   .githooks/one-bevy.sh [package]   (default guacamole)
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
root=$(pwd -P)
pkg=${1:-guacamole}

graph() {
  cargo tree -p "$pkg" --prefix depth -f '{p}|{f}' -e "$1" --color never 2>/dev/null
}

build=$(graph normal,build) || { echo "one-bevy: cargo tree -p $pkg failed" >&2; exit 2; }
tests=$(graph normal,build,dev) || { echo "one-bevy: cargo tree -p $pkg failed" >&2; exit 2; }

awk -v root="$root" -v pkg="$pkg" '
  # One line: <depth><package>|<features>[ (*)]; a line at depth d is a
  # dependency of the last line seen at depth d-1.
  function node(line, g,    d, rest, bar, id, f) {
    match(line, /^[0-9]+/)
    d = substr(line, 1, RLENGTH) + 0
    rest = substr(line, RLENGTH + 1)
    sub(/ \(\*\)$/, "", rest)
    bar = index(rest, "|")
    id = substr(rest, 1, bar - 1)
    f = substr(rest, bar + 1)
    if (!((g, id) in feats)) feats[g, id] = f
    else if (index(";" feats[g, id] ";", ";" f ";") == 0) feats[g, id] = feats[g, id] ";" f
    seen[g, id] = 1
    ids[id] = 1
    at[d] = id
    if (d > 0 && g == "t") up[id, at[d - 1]] = 1
  }
  FNR == 1 { g = (g == "") ? "b" : "t" }
  { node($0, g) }
  function ours(id) { return index(id, "(" root "/") > 0 }
  function shared(id) { return (("b", id) in seen) && (("t", id) in seen) }
  END {
    for (id in ids)
      if (shared(id) && !ours(id) && feats["b", id] != feats["t", id]) differ[id] = 1
    # Every shared third-party crate above a differing one, in the tests graph.
    for (id in differ) forked[id] = 1
    do {
      grew = 0
      for (k in up) {
        split(k, p, SUBSEP)
        if ((p[1] in forked) && !(p[2] in forked) && shared(p[2]) && !ours(p[2])) {
          forked[p[2]] = 1; grew = 1
        }
      }
    } while (grew)
    bad = 0
    for (id in differ) {
      n = 0; eg = ""
      for (a in forked) {
        if (a in differ) continue
        # a is above id when id reaches a through the up edges
        if (reaches(id, a)) { n++; if (n <= 4) eg = eg (eg == "" ? "" : ", ") a }
      }
      if (n > 0) {
        bad = 1
        printf "one-bevy: %s builds with different features for `cargo build -p %s` and its tests, so %d crate(s) above it build twice (%s%s)\n", id, pkg, n, eg, (n > 4 ? ", ..." : "") > "/dev/stderr"
        printf "  build: %s\n  tests: %s\n", feats["b", id], feats["t", id] > "/dev/stderr"
      } else {
        printf "one-bevy: %s builds twice by itself (only our or test-only crates depend on it)\n", id
      }
    }
    if (bad) {
      print "one-bevy: name it in crates/" pkg "/Cargo.toml with the tests features (see the rustix line there)" > "/dev/stderr"
      exit 1
    }
    print "one-bevy: " pkg "'"'"'s build and tests share one Bevy"
  }
  function reaches(from, to,    k, p, q, n, head, cur, mark) {
    delete mark
    n = 1; q[1] = from; head = 1; mark[from] = 1
    while (head <= n) {
      cur = q[head++]
      for (k in up) {
        split(k, p, SUBSEP)
        if (p[1] == cur && !(p[2] in mark) && (p[2] in forked)) {
          if (p[2] == to) return 1
          mark[p[2]] = 1; q[++n] = p[2]
        }
      }
    }
    return 0
  }
' <(printf '%s\n' "$build") <(printf '%s\n' "$tests")
