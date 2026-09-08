#!/bin/bash
#
# Answer one question the crate cannot answer from its own test suite: does the
# tmux buffer round-trip preserve bytes on this tmux version?
#
# tmux 3.4 and 3.5 pass command output through `utf8_stravisx()` with
# VIS_OCTAL|VIS_CSTYLE|VIS_NOSLASH before writing it to the client. Earlier and
# later versions emit raw bytes. `show-buffer` writes through that same sink,
# so if it is escaped there, `capture-pane -b` + `show-buffer` cannot be used
# as a length-framed transport for pane contents — which is what a future
# control-mode transport wants it for.
#
# Two checks, both fatal: load known bytes into a buffer and require the
# readback and the advertised `#{buffer_size}` to match the input exactly, then
# read one pane both ways and require the two routes to agree.
#
# Every released tmux answers this identically forever, so the rows in the
# matrix are settled; the point is that a version added later answers it
# without anyone having to remember the question exists.

set -euo pipefail

if ! command -v tmux >/dev/null; then
  echo "tmux not found" >&2
  exit 1
fi

socket="wire-probe-$$"
work=$(mktemp -d)
# `kill-server` leaves the socket file behind, so remove it too — this script
# is meant to be runnable on a developer's machine, not only on a runner that
# is thrown away afterwards.
socket_dir="${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"
trap 'tmux -L "$socket" kill-server 2>/dev/null || true; rm -f "$socket_dir/$socket"; rm -rf "$work"' EXIT

# Bytes chosen for what an escaping sink would visibly change: multi-byte
# UTF-8, a literal backslash (VIS_NOSLASH leaves it alone, so a doubled one
# would show), and an ESC that VIS_CSTYLE would render as `\e`.
printf 'plain ascii\n\xcf\x80 pi and \\ backslash\ncaf\xc3\xa9 \x1b[1m bold\n' \
  >"$work/payload"

tmux -u -L "$socket" new-session -d -s probe
tmux -u -L "$socket" load-buffer -b probe "$work/payload"
tmux -u -L "$socket" show-buffer -b probe >"$work/readback"
advertised=$(tmux -u -L "$socket" list-buffers -F '#{buffer_size}')

written=$(wc -c <"$work/payload" | tr -d ' ')
readback=$(wc -c <"$work/readback" | tr -d ' ')

echo "tmux version:  $(tmux -V)"
echo "written:       $written bytes"
echo "read back:     $readback bytes"
echo "buffer_size:   $advertised"

status=0

if cmp -s "$work/payload" "$work/readback"; then
  echo "PASS  show-buffer round-trips the bytes unchanged"
else
  echo "FAIL  show-buffer altered the bytes; this tmux escapes command output"
  echo "--- read back ---"
  od -c "$work/readback"
  status=1
fi

if [ "$advertised" = "$written" ]; then
  echo "PASS  buffer_size matches the payload length"
else
  echo "FAIL  buffer_size ($advertised) != payload length ($written)"
  status=1
fi

# The two ways to read a pane must agree. `capture-pane -p` writes through the
# command-output sink — the one that vis-escapes on 3.4 and 3.5 — while
# `capture-pane -b` plus `show-buffer` goes by way of a buffer. A divergence is
# what would keep pane capture on the spawning transport.

# Wait up to ten seconds for a condition rather than guessing a sleep, and fail
# the probe when it never holds: a pane that never echoed yields two empty
# captures, which compare equal and would report agreement without having
# compared anything.
wait_for() {
  local description=$1
  shift
  local deadline=$((SECONDS + 10))

  until "$@"; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "FAIL  timed out waiting for $description"
      exit 1
    fi
    sleep 0.2
  done
}

pane_runs_cat() {
  [ "$(tmux -u -L "$socket" display-message -p -t compare '#{pane_current_command}')" = cat ]
}

pane_echoed() {
  tmux -u -L "$socket" capture-pane -p -t compare | grep -q backslash
}

tmux -u -L "$socket" set-option -g default-command "cat" >/dev/null
tmux -u -L "$socket" new-window -d -n compare

# Keys sent before `cat` owns the tty are dropped, so wait for the pane to be
# running it before typing into it.
wait_for "the comparison pane to start cat" pane_runs_cat
tmux -u -L "$socket" send-keys -t compare 'π café \ backslash' Enter
wait_for "the comparison pane to echo the line" pane_echoed

tmux -u -L "$socket" capture-pane -p -t compare >"$work/direct"
tmux -u -L "$socket" capture-pane -b compare -t compare
tmux -u -L "$socket" show-buffer -b compare >"$work/buffered"

if cmp -s "$work/direct" "$work/buffered"; then
  echo "PASS  capture-pane -p and capture-pane -b agree byte for byte"
else
  echo "FAIL  capture-pane -p and capture-pane -b diverge on this version"
  diff <(od -c "$work/direct") <(od -c "$work/buffered") || true
  status=1
fi

exit $status
