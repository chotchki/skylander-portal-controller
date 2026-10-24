#!/usr/bin/env bash
# No doctest goes unrun (openspec AQ.36): the workspace's libraries set
# `doctest = false`, since they hold none and `cargo test --workspace` spent
# 4.4 s a run (13.3 s after a rebuild) on 16 empty doc-test runs (AQ.4). That
# key makes a doctest added later skip silently, so this fails instead, naming
# the line: every module file a `doctest = false` library reaches (`mod x;`
# from its root, `#[cfg(test)]` modules left out, as rustdoc leaves them) is
# read for a doc comment code block rustdoc would run, an unfenced indented
# block or a fence with no language, `rust` or one of rustdoc's attributes
# (`ignore`, `no_run`, ...). An item's doc is its `///` (or `//!`) lines and
# `/** */` (or `/*! */`) blocks together, unindented by their common indent
# as rustdoc does, so a doc that opens on an indented block is one. A
# `#![doc = include_str!(...)]` file is read the same way and any other
# `#[doc = ...]` fails, since it can't be read here.
# Errs loud: a block that wouldn't run fails too. Fence it with its language
# (```text, ```sh) or drop the library's `doctest = false` so it runs.
# (`cargo test --doc` runs a library's doctests whatever its key, so a CI or
# by-hand run of it finds one too; this is the cheap check, 0.5 s.)
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

cargo metadata --no-deps --format-version 1 --color never |
  jq -r '.packages[] | .name as $p | .targets[]
         | select(any(.kind[]; . == "lib") and .doctest == false)
         | "\($p)\t\(.src_path)"' |
  awk -F'\t' '
    { pkgs[++n] = $1; roots[n] = $2 }
    function rusty(info,    k, t, i, other) {
      k = split(info, t, /[ \t,]+/)
      other = 0
      for (i = 1; i <= k; i++) {
        if (t[i] == "") continue
        if (t[i] ~ /^(rust|ignore(-.*)?|should_panic|no_run|compile_fail|test_harness|standalone_crate|edition[0-9]+|E[0-9]+)$/) return 1
        other = 1
      }
      return !other
    }
    function hit(what) {
      printf "no-doctests: %s:%d: %s in %s, whose [lib] sets doctest = false\n", file, at, what, pkg > "/dev/stderr"
      bad = 1
    }
    function doc(c) {                      # one line of an item'"'"'s markdown
      if (fence != "") {
        if (c ~ /^ {0,3}(```+|~~~+)[ \t]*$/ && substr(c, index(c, substr(fence, 1, 1)), length(fence)) == fence) fence = ""
        return
      }
      if (c ~ /^ {0,3}(```|~~~)/) {
        match(c, /(```+|~~~+)/); fence = substr(c, RSTART, RLENGTH)
        info = substr(c, RSTART + RLENGTH)
        if (rusty(info)) hit("a " fence info " block")
        return
      }
      if (c ~ /^[ \t]*$/) { blank = 1; return }
      if (blank && c ~ /^(    |\t)/ && !inlist) hit("an indented code block")
      if (c ~ /^ {0,3}([-*+]|[0-9]+[.)])[ \t]/) inlist = 1
      else if (blank && c !~ /^[ \t]/) inlist = 0
      blank = 0
    }
    # One line of the doc being read, with the line it came from.
    function keep(c, ln) { pend[++np] = c; pendln[np] = ln }
    # The item'"'"'s doc read: its lines unindented by their common indent, as
    # rustdoc does, then read as one markdown text that a code block may open.
    function flush(    i, m) {
      m = -1
      for (i = 1; i <= np; i++)
        if (pend[i] !~ /^[ \t]*$/) { match(pend[i], /^[ \t]*/); if (m < 0 || RLENGTH < m) m = RLENGTH }
      fence = ""; blank = 1; inlist = 0
      for (i = 1; i <= np; i++) { at = pendln[i]; doc(substr(pend[i], m + 1)) }
      np = 0
    }
    # A `/**` or `/*!` block, rest the text after its opener: its lines up to
    # `*/`, trimmed as rustc trims one (a first line of stars or nothing and a
    # last line of stars dropped, and a leading `*` off every line when each
    # but a bare first one has it).
    function block(path, rest,    k, b, bl, i, s, j, star, end) {
      k = 0
      while (1) {
        end = index(rest, "*/")
        b[++k] = end ? substr(rest, 1, end - 1) : rest; bl[k] = lineno
        if (end) break
        if ((getline rest < path) <= 0) break
        lineno++
      }
      i = (b[1] ~ /^\**$/) ? 2 : 1
      if (k >= i && b[k] != "" && b[k] ~ /^\**$/) k--
      s = (i <= k && b[i] !~ /^[ \t]*\*/) ? i + 1 : i
      star = 0
      for (j = s; j <= k; j++) {
        if (b[j] ~ /^[ \t]*$/) continue
        if (b[j] !~ /^[ \t]*\*/) { star = 0; break }
        star = 1
      }
      for (j = i; j <= k; j++) {
        if (star && b[j] ~ /^[ \t]*\*( |$|\*)/) sub(/^[ \t]*\*/, "", b[j])
        keep(b[j], bl[j])
      }
    }
    function dirof(path) { sub(/\/[^\/]*$/, "", path); return path }
    function exists(path,    probe, r) {
      r = (getline probe < path) >= 0
      close(path)
      return r
    }
    function enqueue(path) { if (!(path in queued)) { queued[path] = 1; queue[++tail] = path } }
    # One file: its doc comments read, its `mod x;` children queued.
    function scan(path, md,    line, base, dir, depth, indent, test, names, cfgtest, skipping, c, name, ind, prefix, i, child, inc) {
      file = path; lineno = 0; depth = 0; cfgtest = 0; np = 0
      base = path; sub(/^.*\//, "", base)
      dir = dirof(path)
      if (base != "lib.rs" && base != "main.rs" && base != "mod.rs") dir = dir "/" substr(base, 1, length(base) - 3)
      while ((getline line < path) > 0) {
        lineno++
        if (md) { keep(line, lineno); continue }
        # Inline modules by rustfmt indentation; a #[cfg(test)] one is skipped.
        if (depth > 0 && line ~ /^ *\} *$/) {
          match(line, /^ */)
          if (RLENGTH == indent[depth]) { depth--; flush(); continue }
        }
        skipping = (depth > 0 && test[depth])
        # `///` and `//!`, not `////`; `/**` and `/*!`, not `/***` or `/**/`.
        if (match(line, /^[ \t]*\/\/(\/([^\/]|$)|!)/)) {
          match(line, /^[ \t]*/)
          if (!skipping) keep(substr(line, RLENGTH + 4), lineno)
          continue
        }
        if (match(line, /^[ \t]*\/\*(\*([^*\/]|$)|!)/)) {
          match(line, /^[ \t]*/)
          c = substr(line, RLENGTH + 4)
          if (skipping) { kept = np; block(path, c); np = kept } else block(path, c)
          continue
        }
        if (line ~ /^[ \t]*#!?\[/) {
          if (line ~ /cfg\((all\()?test[,)]/) cfgtest = 1
          if (!skipping && line ~ /#!?\[doc[ \t]*=/) {
            at = lineno
            if (match(line, /include_str!\("[^"]+"\)/)) {
              inc = dirof(path) "/" substr(line, RSTART + 14, RLENGTH - 16)
              if (exists(inc)) mdq[inc] = 1
              else hit("a #[doc = include_str!] of a file not found, " inc)
            } else hit("a #[doc = ...] attribute, not read here")
          }
          continue
        }
        if (line ~ /^[ \t]*$/ || line ~ /^[ \t]*\/\//) continue
        flush()
        if (match(line, /^ *(pub(\([a-z ]+\))? )?mod [A-Za-z_0-9]+ \{/)) {
          match(line, /^ */); ind = RLENGTH
          name = line; sub(/^ *(pub(\([a-z ]+\))? )?mod /, "", name); sub(/ \{.*/, "", name)
          depth++; indent[depth] = ind; test[depth] = cfgtest || (depth > 1 && test[depth - 1]); names[depth] = name
          cfgtest = 0; continue
        }
        if (line ~ /^ *(pub(\([a-z ]+\))? )?mod [A-Za-z_0-9]+;/ && !cfgtest && !skipping) {
          name = line; sub(/^ *(pub(\([a-z ]+\))? )?mod /, "", name); sub(/;.*/, "", name)
          prefix = ""
          for (i = 1; i <= depth; i++) prefix = prefix names[i] "/"
          child = dir "/" prefix name
          at = lineno
          if (exists(child ".rs")) enqueue(child ".rs")
          else if (exists(child "/mod.rs")) enqueue(child "/mod.rs")
          else hit("a module whose file isn'"'"'t found, " child ".rs")
        }
        cfgtest = 0
      }
      flush()
      close(path)
    }
    END {
      for (k = 1; k <= n; k++) {
        pkg = pkgs[k]; delete queued; delete mdq; tail = 0; head = 1
        enqueue(roots[k])
        while (head <= tail) scan(queue[head++], 0)
        for (m in mdq) scan(m, 1)
        files += tail
      }
      if (bad) {
        print "no-doctests: fence the block with its language (```text) or drop that library'"'"'s doctest = false so it runs" > "/dev/stderr"
        exit 1
      }
      printf "no-doctests: %d libraries with doctest = false, %d module files, no doc code block in them\n", n, files
    }
  '
