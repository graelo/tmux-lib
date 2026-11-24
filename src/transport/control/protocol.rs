//! Read one reply from the tmux control stream.
//!
//! ```text
//! %session-changed $0 work
//! %begin 1789161427 1037 1
//! command output
//! %end 1789161427 1037 1
//! ```
//!
//! tmux sends notifications between replies, never inside them. Ignore those
//! lines apart from `%exit`. Inside a reply, only `%end` or `%error` with the
//! exact `<time> <number> <flags>` from `%begin` ends the block. Keep all other
//! bytes as-is: a line in the output can itself start with `%`.

use std::io::{self, BufRead};

#[derive(Debug)]
pub(super) struct Block {
    pub(super) flags: u64,
    pub(super) body: Vec<u8>,
    pub(super) failed: bool,
}

#[derive(Debug)]
pub(super) enum Frame {
    Reply(Block),
    Exit(Option<String>),
}

/// Read a complete frame, reusing `line` as scratch space across calls.
pub(super) fn read_frame(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
) -> io::Result<Option<Frame>> {
    let (triple, flags) = loop {
        if !read_line(reader, line)? {
            return Ok(None);
        }
        let text = &line[..line.len() - 1]; // Exclude the newline for markers.
        if let Some(triple) = text.strip_prefix(b"%begin ")
            && let Some(flags) = flags(triple)
        {
            break (triple.to_vec(), flags);
        }
        if text == b"%exit" {
            return Ok(Some(Frame::Exit(None)));
        }
        if let Some(reason) = text.strip_prefix(b"%exit ") {
            return Ok(Some(Frame::Exit(Some(
                String::from_utf8_lossy(reason).into_owned(),
            ))));
        }
    };

    let mut body = Vec::new();
    while read_line(reader, line)? {
        let text = &line[..line.len() - 1];
        if text.strip_prefix(b"%end ") == Some(triple.as_slice()) {
            return Ok(Some(Frame::Reply(Block {
                flags,
                body,
                failed: false,
            })));
        }
        if text.strip_prefix(b"%error ") == Some(triple.as_slice()) {
            return Ok(Some(Frame::Reply(Block {
                flags,
                body,
                failed: true,
            })));
        }
        body.extend_from_slice(line);
    }
    Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "incomplete tmux control reply",
    ))
}

// A partial line is never a marker, even if it starts like one.
fn read_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<bool> {
    line.clear();
    match reader.read_until(b'\n', line)? {
        0 => Ok(false),
        _ if line.ends_with(b"\n") => Ok(true),
        _ => Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "incomplete tmux control line",
        )),
    }
}

fn flags(triple: &[u8]) -> Option<u64> {
    let mut fields = std::str::from_utf8(triple).ok()?.split(' ');
    fields.next()?.parse::<u64>().ok()?; // time
    fields.next()?.parse::<u64>().ok()?; // command number
    let flags = fields.next()?.parse().ok()?;
    fields.next().is_none().then_some(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(reader: &mut &[u8]) -> Block {
        match read_frame(reader, &mut Vec::new()).unwrap() {
            Some(Frame::Reply(block)) => block,
            other => panic!("expected a reply, got {other:?}"),
        }
    }

    #[test]
    fn reads_attach_and_command_replies_past_notifications() {
        let mut transcript = &b"%begin 1 2 0\n%end 1 2 0\n\
                                %session-changed $0 work\n%begin not a triple\n\
                                %begin 1 3 1\nhello\n%end 1 3 1\n\
                                %begin 1 4 1\nunknown command\n%error 1 4 1\n"[..];
        let attach = block(&mut transcript);
        assert_eq!(attach.flags, 0);
        assert!(attach.body.is_empty());
        assert!(!attach.failed);

        let reply = block(&mut transcript);
        assert_eq!(reply.flags, 1);
        assert_eq!(reply.body, b"hello\n");
        assert!(!reply.failed);

        let failure = block(&mut transcript);
        assert_eq!(failure.body, b"unknown command\n");
        assert!(failure.failed);
        assert!(
            read_frame(&mut transcript, &mut Vec::new())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn keeps_marker_like_lines_and_non_utf8_bytes_in_the_body() {
        let reply = block(
            &mut &b"%begin 1 2 1\n\
                                  %end 1 2 1 trailing text\n%error 1 3 1\n%begin 1 4 1\n\
                                  %exit\n\xff\n%end 1 2 1\n"[..],
        );
        assert_eq!(
            reply.body,
            b"%end 1 2 1 trailing text\n%error 1 3 1\n%begin 1 4 1\n%exit\n\xff\n"
        );
    }

    #[test]
    fn reports_exit_with_or_without_a_reason() {
        let mut line = Vec::new();
        assert!(matches!(
            read_frame(&mut &b"%exit\n"[..], &mut line).unwrap(),
            Some(Frame::Exit(None))
        ));
        assert!(matches!(
            read_frame(&mut &b"%exit server exited\n"[..], &mut line).unwrap(),
            Some(Frame::Exit(Some(reason))) if reason == "server exited"
        ));
    }

    #[test]
    fn never_returns_a_partial_reply() {
        for transcript in [
            b"%begin 1 2 1\npayload\n".as_slice(),
            b"%begin 1 2 1\n%end 1 2 1",
        ] {
            let error = read_frame(&mut &transcript[..], &mut Vec::new()).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        }
    }
}
