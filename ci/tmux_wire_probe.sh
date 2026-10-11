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
# Five checks, all fatal: load known bytes into a buffer and require the
# readback and the advertised `#{buffer_size}` to match the input exactly, read
# one pane both ways and require the two routes to agree, set an option over a
# control connection and require the argument to arrive unchanged, length-frame
# a capture over a control connection, and require a control connection to
# report the same window layouts as a forked client.
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
# The flags the crate attaches its control client with.
attach_flags=no-output,ignore-size,new-layouts
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

# Can an argument survive tmux's command lexer on a control connection? The
# control transport sends one command per line, so every argument is
# single-quoted, and an embedded single quote is written the way a shell writes
# it — close, escape, reopen. That relies on tmux processing a backslash escape
# outside quotes and no escape at all inside them.
#
# Each value is set twice: once over the control connection, and once by
# forking a client, where the argument never meets the lexer. The two readbacks
# are then compared against each other rather than against the literal, so that
# a version escaping its *output* — which 3.4 and 3.5 do — cannot be mistaken
# for a quoting failure on the way in.

# Single-quote one argument the way the crate's `quoting` module does.
sq() {
  local escaped=${1//\'/\'\\\'\'}
  printf "'%s'" "$escaped"
}

names=(plain spaces single double backslash dollar semicolon hash utf8 format trailing)
values=(
  'simple'
  'two words'
  "it's here"
  'say "hi"'
  'back\slash'
  '$HOME and #{pane_id}'
  'a ; b'
  '# comment?'
  'π café'
  '#{?a,b,c}'
  'ends with \'
)

for i in "${!names[@]}"; do
  printf 'set-option -g @ctrl_%s %s\n' "${names[i]}" "$(sq "${values[i]}")"
done | tmux -u -L "$socket" -C attach -f "$attach_flags" >"$work/control" 2>&1

for i in "${!names[@]}"; do
  tmux -u -L "$socket" set-option -g "@spawn_${names[i]}" "${values[i]}"
done

quoting_ok=1
for i in "${!names[@]}"; do
  via_control=$(tmux -u -L "$socket" show-options -g -v "@ctrl_${names[i]}" 2>&1 || true)
  via_spawn=$(tmux -u -L "$socket" show-options -g -v "@spawn_${names[i]}" 2>&1 || true)

  if [ "$via_control" != "$via_spawn" ]; then
    echo "FAIL  control-mode quoting altered '${names[i]}'"
    echo "        via control: $(printf '%s' "$via_control" | od -c | head -3)"
    echo "        via spawn:   $(printf '%s' "$via_spawn" | od -c | head -3)"
    quoting_ok=0
  fi
done

if [ "$quoting_ok" = 1 ]; then
  echo "PASS  every argument survives the control-mode command lexer"
else
  echo "--- control connection transcript ---"
  cat "$work/control"
  status=1
fi

# Can a capture be length-framed on a control connection? The checks above
# read buffers by forking a client; a control client writes command output
# through its own path, which is the one that matters for moving captures onto
# the connection. Capture into a buffer and read it back over the connection,
# with the flags the crate captures with, then require the block body to be
# exactly `#{buffer_size}` plus the one newline control mode adds, and the
# payload to match a capture taken by forking a client.

capture_flags=(-J -e -S - -E -)

# A pane holding what an escaping sink would change — colour escapes, which
# `-e` keeps as ESC bytes, and multi-byte UTF-8 — plus a line shaped like a
# block terminator, which only the exact-triple match keeps inside the body.
tmux -u -L "$socket" new-window -d -n framed \
  "printf '\\033[31mred\\033[0m π %%end 1 2 3 BBB\\nframed-done\\n'; exec cat"

pane_printed() {
  tmux -u -L "$socket" capture-pane -p -t framed | grep -q framed-done
}
wait_for "the framing pane to print" pane_printed

tmux -u -L "$socket" capture-pane -p "${capture_flags[@]}" -t framed \
  >"$work/direct-full"

printf '%s\n' \
  "capture-pane -b ctlcap ${capture_flags[*]} -t framed" \
  'show-buffer -b ctlcap' |
  tmux -u -L "$socket" -C attach -f "$attach_flags" \
    >"$work/control-capture" 2>&1

ctl_size=$(tmux -u -L "$socket" list-buffers -F '#{buffer_name} #{buffer_size}' |
  awk '$1 == "ctlcap" { print $2 }')

# Print the body of the second command's block, matching its terminator by the
# exact `%begin` triple as the crate does. Exit 2 if tmux reported an error.
LC_ALL=C awk '
  open == "" && $1 == "%begin" && $4 == "1" { n++; open = $2 " " $3 " " $4; next }
  open != "" && $0 == "%error " open { if (n == 2) exit 2; open = ""; next }
  open != "" && $0 == "%end " open { if (n == 2) exit 0; open = ""; next }
  open != "" && n == 2 { print }
' "$work/control-capture" >"$work/ctl-body" || {
  echo "FAIL  show-buffer failed on the control connection"
  cat "$work/control-capture"
  status=1
}

ctl_body=$(wc -c <"$work/ctl-body" | tr -d ' ')
echo "buffer_size:   ${ctl_size:-missing} (control capture)"
echo "block body:    $ctl_body bytes"

if [ -n "$ctl_size" ] && [ "$ctl_body" -eq $((ctl_size + 1)) ]; then
  echo "PASS  a control-mode show-buffer body is buffer_size + 1 bytes"
else
  echo "FAIL  a control-mode show-buffer body is not buffer_size + 1 bytes"
  status=1
fi

if [ -n "$ctl_size" ] &&
  head -c "$ctl_size" "$work/ctl-body" | cmp -s - "$work/direct-full"; then
  echo "PASS  a buffered capture over control matches a forked capture"
else
  echo "FAIL  a buffered capture over control differs from a forked capture"
  diff <(od -c "$work/direct-full") <(od -c "$work/ctl-body") || true
  status=1
fi

# Does a control connection report the same layouts as a forked client? tmux
# 3.8 reports a new layout format, but to a control client only when it
# attaches with `new-layouts`; without it, the control client gets the older
# format, which leaves floating panes out. Earlier versions must accept the
# flag and carry on sending the one format they have.

tmux -u -L "$socket" new-window -d -n layout
tmux -u -L "$socket" split-window -d -h -t layout
tmux -u -L "$socket" split-window -d -v -t layout

layout_format='#{window_id} #{window_layout}'
tmux -u -L "$socket" list-windows -F "$layout_format" >"$work/layouts-forked"

printf 'list-windows -F %s\n' "$(sq "$layout_format")" |
  tmux -u -L "$socket" -C attach -f "$attach_flags" \
    >"$work/control-layouts" 2>&1

# The body of the first command's block, as above.
LC_ALL=C awk '
  open == "" && $1 == "%begin" && $4 == "1" { n++; open = $2 " " $3 " " $4; next }
  open != "" && $0 == "%error " open { exit 2 }
  open != "" && $0 == "%end " open { exit 0 }
  open != "" { print }
' "$work/control-layouts" >"$work/layouts-control" || {
  echo "FAIL  list-windows failed on the control connection"
  cat "$work/control-layouts"
  status=1
}

if [ -s "$work/layouts-forked" ] &&
  cmp -s "$work/layouts-forked" "$work/layouts-control"; then
  echo "PASS  a control connection reports the same layouts as a forked client"
else
  echo "FAIL  a control connection reports different layouts"
  echo "--- forked ---"
  cat "$work/layouts-forked"
  echo "--- control ---"
  cat "$work/layouts-control"
  status=1
fi

exit $status
