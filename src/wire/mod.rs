//! The tmux wire protocol: what the bytes mean.
//!
//! This module owns every detail of how tmux is asked for data and how the
//! reply is framed — format strings, the `\x1f` separator, byte-length
//! prefixes, backslash doubling, and the `vis(3)` escaping of tmux 3.4 and
//! 3.5. Nothing outside it needs to know any of that; model types receive a
//! [`RecordReader`] and read their own fields by meaning.

pub(crate) mod field;
pub(crate) mod formats;
pub(crate) mod framing;
pub(crate) mod record;

pub(crate) use framing::{ByteParseError, normalize_tmux_output};
pub(crate) use record::{RecordReader, decode_all, decode_one};
