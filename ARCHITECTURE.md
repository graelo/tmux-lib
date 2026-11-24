# Architecture

## One handle, one server

A `Tmux` handle chooses a server address and a transport. The Rust process
never opens the tmux socket itself. It starts a `tmux` client, and that client
connects to the server.

```text
Rust process                   tmux client process           tmux server

spawning handle -- argv -----> tmux (one per command) <-- Unix socket --> server
                <- stdout, stderr, exit status

control handle  -- stdin ----> tmux -C attach         <-- Unix socket --> server
                <- stdout ---- (reused across calls)
```

`Server::Default` lets tmux resolve the socket normally. `SocketName` passes
`-L <name>`; `SocketPath` passes `-S <path>`. Both transports also pass `-u`
to force UTF-8 even when the caller's locale is unset.

`Tmux::control_on` starts `tmux -C attach` immediately. The server must
already have a session to attach to. A control command is an ordinary tmux
command sent to that client's stdin instead of starting a new process. For
example, `available_sessions()` sends `list-sessions -F …`, and
`select_pane()` sends `select-pane -t %3` when targeting pane `%3`.

When the crate starts the control client, it also starts a Rust thread to
read the child's stdout. tmux can send notifications even when no caller
is waiting, so the thread keeps the pipe from filling. It discards those
notifications and forwards replies to the caller. The `no-output` attach
flag suppresses live pane output, not notifications or command replies.
The constructor waits for the attach reply before returning the handle.

Cloning the `Tmux` handle does not start another client. All clones share
the same child and mutex: the caller holds the lock from sending a command
until its reply arrives, so two calls cannot claim the same reply.
`disconnect()` releases the client for all clones. After a disconnect or
connection failure, the next command sent over control attaches again. The
crate does not retry a failed call: the command may already have run.

Some methods deliberately call `run_spawned` even on a control handle.
The one-off client connects to the **same server**, but takes arguments as
an argument vector instead of parsing a line on stdin. That lets
`new_window()` pass a working directory containing a newline. It also keeps
`kill_session()` from waiting for a reply on the control client that the
command may be about to detach. This is a choice made before sending the
command, not a fallback after failure.

## Follow `available_sessions()`

[`ops/session.rs`](src/ops/session.rs) runs `list-sessions -F <format>`.
`-F` tells tmux exactly what to print for each session. The transport
returns a `Reply`: whether the command failed, its output bytes, and any
diagnostic. `available_sessions()` checks for failure before parsing, so
an error from tmux doesn't look like a malformed session record.

```text
                         list-sessions -F <format>
                                     |
                  +------------------+-----------------+
                  |                                    |
           spawn: argv                     control: quote argv for tmux,
                  |                         send one line on stdin
                  v                                    v
       stdout + stderr + status             %begin ... %end / %error
                  |                                    |
                  |                        strip the control envelope
                  +------------------+-----------------+
                                     v
                                   Reply
                                     | output(): success body bytes
                                     v
                    decode_all(SESSION_FIELDS, Session::decode)
                                     |
                                     v
                                Vec<Session>
```

**Where does the reply end?** A spawned client runs the command and exits,
so its stdout is already separate from other commands' output. The control
client stays open, so tmux wraps each reply in `%begin` and `%end` (or
`%error` on failure). The parser matches the full `<time> <number> <flags>`
triple from `%begin`: a line that merely starts with `%end` might be part
of the output. On `%error`, the block's body becomes the diagnostic, like
stderr on the spawning path. Between replies, a notification such as
`%session-changed` is discarded; `%exit` signals that the client is leaving.

The control writer single-quotes arguments for tmux's lexer, not a shell.
For example, a target containing a space must arrive as one argument.
Arguments containing a newline are rejected before anything is sent.

**How do the bytes become a `Session`?** `SESSION_FIELDS` lists the ID,
name, and path in order. The crate uses that list twice: to build the `-F`
format and to read the output. An ID like `$0` is read up to the next
separator. A name or path can contain the separator, so `#{n:session_name}`
asks tmux to print its byte length before the value. In this example,
`work` and `/tmp` are each four bytes. `<US>` marks one byte (`0x1f`):

```text
format:  #{session_id}<US>#{n:session_name}<US>#{session_name}<US>#{n:session_path}<US>#{session_path}

%begin 1789161427 1037 1             <- control envelope
$0<US>4<US>work<US>4<US>/tmp         <- same record bytes as spawned stdout
%end 1789161427 1037 1               <- control envelope
```

The same decoder reads the record on either transport. It takes the stated
number of bytes before looking for the next separator or, after the last
field, a newline. A newline inside a path is therefore just data.
Backslashes are literal; the record reader doesn't unescape them.

## Why control captures use a buffer

`capture_pane()` returns screen bytes, not records. On a spawning handle,
`capture-pane -p` writes those bytes to stdout. On a control handle, pane
text could contain a line identical to the full `%end` marker, and the
reader would mistake it for the end of the reply.

The control path runs `capture-pane -b` to save the capture in a temporary
tmux paste buffer. `list-buffers` gives its size before `show-buffer` reads
it. The reply must contain that many bytes plus one newline from tmux. A
short reply is an error instead of a silently truncated capture. The method
then runs `delete-buffer`, even if reading the buffer failed.

## Where to change what

- **Command routing:** [`tmux.rs`](src/tmux.rs) defines the handle and server
  address. [`ops/`](src/ops/) builds arguments and handles each command's
  reply. [`transport/mod.rs`](src/transport/mod.rs) selects the transport and
  builds the address arguments shared by both.
- **Transport:** [`spawning.rs`](src/transport/spawning.rs) starts a client per
  call. [`control/mod.rs`](src/transport/control/mod.rs) owns the persistent
  child, mutex, and reader thread. Next to it,
  [`quoting.rs`](src/transport/control/quoting.rs) writes command lines and
  [`protocol.rs`](src/transport/control/protocol.rs) reads reply blocks.
  Both transports return the [`Reply`](src/transport/reply.rs) type.
- **Record format:** [`wire/formats.rs`](src/wire/formats.rs) declares each
  field list; [`field.rs`](src/wire/field.rs) renders it as `-F`.
  [`record.rs`](src/wire/record.rs) reads fields using
  [`framing.rs`](src/wire/framing.rs) for byte-level reads. The decoders in
  [`state/`](src/state/) turn those fields into resource types and IDs.
