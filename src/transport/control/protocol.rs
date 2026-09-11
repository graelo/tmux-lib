//! The control-mode line protocol: replies in, events out.
//!
//! tmux answers each command with a block:
//!
//! ```text
//! %begin 1789161427 1037 1
//! parse error: unknown command: nosuchcommand
//! %error 1789161427 1037 1
//! ```
//!
//! `%end` closes a block that succeeded and `%error` one that failed, both
//! repeating the `<time> <number> <flags>` triple of their `%begin`. Anything
//! outside a block is a notification. Observed on tmux 3.7c.
//!
//! Two facts make a sequential demux correct, and both are load-bearing:
//!
//! - A notification never lands inside a block, so inside one, every line that
//!   is not the terminator is payload.
//! - A block is closed only by its own exact triple. Since tmux 3.6 nothing
//!   escapes command output, so a pane printing `%end 1 2 3 BBB` has that line
//!   land verbatim inside its own capture. Matching a bare `%end` prefix would
//!   truncate the capture there and desynchronise every later reply.

/// One command's reply, as the terminator reported it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Block {
    /// The `flags` field of the block's triple.
    ///
    /// The attach performs a command of its own before ours, and its block is
    /// the only one carrying `0`. That is how the connect routine recognises
    /// and consumes it rather than handing it to the first caller.
    pub(crate) flags: u64,
    /// Everything tmux printed between the `%begin` and the terminator, with
    /// each line's newline kept, so an empty reply is empty rather than a
    /// lone newline.
    pub(crate) body: Vec<u8>,
    /// Whether the terminator was `%error` rather than `%end`.
    pub(crate) failed: bool,
}

/// What one line of the control stream turned out to mean.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// A command's block closed.
    Block(Block),
    /// A line outside any block: tmux reporting that something changed.
    Notification(Vec<u8>),
    /// The control client is going away, with tmux's reason when it gave one.
    Exit(Option<String>),
}

/// The block currently being accumulated.
struct Open {
    /// The bytes after `%begin `, matched verbatim against a terminator's.
    triple: Vec<u8>,
    flags: u64,
    body: Vec<u8>,
}

/// Turns control-mode lines into [`Event`]s.
///
/// Fed one line at a time with its trailing newline removed, so that the
/// caller can own the reading and this can own the meaning.
#[derive(Default)]
pub(crate) struct Demux {
    open: Option<Open>,
}

impl Demux {
    pub(crate) fn new() -> Demux {
        Demux::default()
    }

    /// Interpret one line, returning an event when it completes one.
    pub(crate) fn line(&mut self, line: &[u8]) -> Option<Event> {
        match self.open.as_mut() {
            Some(open) => {
                let failed = if terminates(line, b"%end ", &open.triple) {
                    false
                } else if terminates(line, b"%error ", &open.triple) {
                    true
                } else {
                    open.body.extend_from_slice(line);
                    open.body.push(b'\n');
                    return None;
                };

                let open = self.open.take()?;
                Some(Event::Block(Block {
                    flags: open.flags,
                    body: open.body,
                    failed,
                }))
            }
            None => self.outside_a_block(line),
        }
    }

    fn outside_a_block(&mut self, line: &[u8]) -> Option<Event> {
        if let Some(triple) = line.strip_prefix(b"%begin ")
            && let Some(flags) = flags_of(triple)
        {
            self.open = Some(Open {
                triple: triple.to_vec(),
                flags,
                body: Vec::new(),
            });
            return None;
        }

        if line == b"%exit" {
            return Some(Event::Exit(None));
        }
        if let Some(reason) = line.strip_prefix(b"%exit ") {
            return Some(Event::Exit(Some(
                String::from_utf8_lossy(reason).into_owned(),
            )));
        }

        Some(Event::Notification(line.to_vec()))
    }
}

/// Whether `line` is exactly `keyword` followed by this block's own triple.
fn terminates(line: &[u8], keyword: &[u8], triple: &[u8]) -> bool {
    line.strip_prefix(keyword) == Some(triple)
}

/// The `flags` field of a `<time> <number> <flags>` triple.
///
/// `None` when the line is not a triple at all, which keeps a notification
/// that happens to start with `%begin ` from opening a block.
fn flags_of(triple: &[u8]) -> Option<u64> {
    let triple = std::str::from_utf8(triple).ok()?;
    let mut fields = triple.split(' ');

    let _time: u64 = fields.next()?.parse().ok()?;
    let _number: u64 = fields.next()?.parse().ok()?;
    let flags = fields.next()?.parse().ok()?;

    fields.next().is_none().then_some(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a transcript line by line, collecting what it produced.
    fn events(transcript: &[u8]) -> Vec<Event> {
        let mut demux = Demux::new();
        transcript
            .split(|&b| b == b'\n')
            .filter_map(|line| demux.line(line))
            .collect()
    }

    fn block(events: Vec<Event>) -> Block {
        let [Event::Block(block)] = <[Event; 1]>::try_from(events).expect("expected one event")
        else {
            panic!("expected a block");
        };
        block
    }

    #[test]
    fn a_successful_command_yields_its_output() {
        let reply = block(events(
            b"%begin 1789161427 1036 1\nprobe\n%end 1789161427 1036 1",
        ));

        assert_eq!(reply.body, b"probe\n");
        assert!(!reply.failed);
        assert_eq!(reply.flags, 1);
    }

    #[test]
    fn a_failed_command_yields_its_message_and_says_so() {
        let reply = block(events(
            b"%begin 1789161427 1037 1\n\
              parse error: unknown command: nosuchcommand\n\
              %error 1789161427 1037 1",
        ));

        assert_eq!(reply.body, b"parse error: unknown command: nosuchcommand\n");
        assert!(reply.failed);
    }

    #[test]
    fn a_command_that_printed_nothing_has_an_empty_body() {
        // `display-message` answers with silence. An empty body must not
        // become a lone newline, or a caller checking for silence is wrong.
        let reply = block(events(b"%begin 1 2 1\n%end 1 2 1"));

        assert!(reply.body.is_empty());
    }

    #[test]
    fn the_attachs_own_block_is_recognisable_by_its_flags() {
        // The full opening of a real attach, from tmux 3.7c.
        let transcript = b"%begin 1789161427 1031 0\n\
                           %end 1789161427 1031 0\n\
                           %session-changed $0 probe\n\
                           %begin 1789161427 1036 1\n\
                           probe\n\
                           %end 1789161427 1036 1";

        let events = events(transcript);
        let [
            Event::Block(attach),
            Event::Notification(changed),
            Event::Block(ours),
        ] = <[Event; 3]>::try_from(events).expect("expected three events")
        else {
            panic!("expected block, notification, block");
        };

        assert_eq!(attach.flags, 0);
        assert!(attach.body.is_empty());
        assert_eq!(changed, b"%session-changed $0 probe");
        assert_eq!(ours.flags, 1);
        assert_eq!(ours.body, b"probe\n");
    }

    #[test]
    fn output_that_looks_like_a_terminator_stays_payload() {
        // Since tmux 3.6 command output is not escaped, so a pane printing
        // this lands verbatim inside its own capture. Matching a bare `%end`
        // prefix would truncate the capture and desynchronise every later
        // reply.
        let reply = block(events(
            b"%begin 1789161427 1040 1\n\
              AAA\n\
              %end 1 2 3 BBB\n\
              %error 7 8 9\n\
              %begin 4 5 6\n\
              CCC\n\
              %end 1789161427 1040 1",
        ));

        assert_eq!(
            reply.body,
            b"AAA\n%end 1 2 3 BBB\n%error 7 8 9\n%begin 4 5 6\nCCC\n"
        );
        assert!(!reply.failed);
    }

    #[test]
    fn a_notification_inside_a_block_would_be_payload() {
        // Stated as a test because the whole demux rests on tmux never doing
        // this: if it ever did, the line would silently corrupt a reply.
        let reply = block(events(
            b"%begin 1 2 1\nbefore\n%sessions-changed\nafter\n%end 1 2 1",
        ));

        assert_eq!(reply.body, b"before\n%sessions-changed\nafter\n");
    }

    #[test]
    fn notifications_between_blocks_are_reported_separately() {
        let events = events(
            b"%unlinked-window-add @1\n\
              %sessions-changed\n\
              %begin 1 2 1\n\
              ok\n\
              %end 1 2 1",
        );

        assert_eq!(
            events,
            vec![
                Event::Notification(b"%unlinked-window-add @1".to_vec()),
                Event::Notification(b"%sessions-changed".to_vec()),
                Event::Block(Block {
                    flags: 1,
                    body: b"ok\n".to_vec(),
                    failed: false,
                }),
            ]
        );
    }

    #[test]
    fn exit_is_reported_with_and_without_a_reason() {
        assert_eq!(events(b"%exit"), vec![Event::Exit(None)]);
        assert_eq!(
            events(b"%exit server exited"),
            vec![Event::Exit(Some("server exited".to_owned()))]
        );
    }

    #[test]
    fn an_exit_inside_a_block_is_payload_not_an_exit() {
        let reply = block(events(b"%begin 1 2 1\n%exit\n%end 1 2 1"));

        assert_eq!(reply.body, b"%exit\n");
    }

    #[test]
    fn a_malformed_begin_does_not_open_a_block() {
        // Without the triple check, a notification starting with `%begin `
        // would swallow every following line as payload.
        assert_eq!(
            events(b"%begin not a triple"),
            vec![Event::Notification(b"%begin not a triple".to_vec())]
        );
        assert_eq!(
            events(b"%begin 1 2"),
            vec![Event::Notification(b"%begin 1 2".to_vec())]
        );
        assert_eq!(
            events(b"%begin 1 2 3 4"),
            vec![Event::Notification(b"%begin 1 2 3 4".to_vec())]
        );
    }

    #[test]
    fn a_body_may_hold_bytes_that_are_not_utf8() {
        // Pane content is arbitrary bytes, so the body is never decoded here.
        let mut transcript = b"%begin 1 2 1\n".to_vec();
        transcript.extend_from_slice(&[0xff, 0xfe, b'\n']);
        transcript.extend_from_slice(b"%end 1 2 1");

        assert_eq!(block(events(&transcript)).body, [0xff, 0xfe, b'\n']);
    }

    #[test]
    fn nothing_is_reported_until_the_block_closes() {
        // A caller blocks on the reply, so a block that never closes must not
        // produce a partial one.
        let mut demux = Demux::new();

        assert_eq!(demux.line(b"%begin 1 2 1"), None);
        assert_eq!(demux.line(b"partial"), None);
        assert!(demux.line(b"%end 1 2 1").is_some());
    }
}
