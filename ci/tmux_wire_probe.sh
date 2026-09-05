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
# The check: load known bytes into a buffer, read them back, and require the
# bytes and the advertised `#{buffer_size}` to both match the input exactly.

set -euo pipefail

if ! command -v tmux >/dev/null; then
  echo "tmux not found" >&2
  exit 1
fi

socket="wire-probe-$$"
work=$(mktemp -d)
trap 'tmux -L "$socket" kill-server 2>/dev/null || true; rm -rf "$work"' EXIT

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

# Informational: the two ways to read a pane should agree. If they diverge on
# some version, the buffer route is the one worth keeping.
tmux -u -L "$socket" set-option -g default-command "cat" >/dev/null
tmux -u -L "$socket" new-window -d -n compare
sleep 0.5
tmux -u -L "$socket" send-keys -t compare 'π café \ backslash' Enter
sleep 0.5
tmux -u -L "$socket" capture-pane -p -t compare >"$work/direct"
tmux -u -L "$socket" capture-pane -b compare -t compare
tmux -u -L "$socket" show-buffer -b compare >"$work/buffered"

if ! grep -q backslash "$work/direct"; then
  # Two empty captures compare equal, which would report agreement without
  # having compared anything. Say so instead.
  echo "INFO  the comparison pane never echoed; capture routes not compared"
elif cmp -s "$work/direct" "$work/buffered"; then
  echo "INFO  capture-pane -p and capture-pane -b agree byte for byte"
else
  echo "INFO  capture-pane -p and capture-pane -b DIVERGE on this version"
  diff <(od -c "$work/direct") <(od -c "$work/buffered") || true
fi

exit $status
