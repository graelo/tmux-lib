//! The tmux wire protocol: what the bytes mean.
//!
//! This module owns every detail of how tmux is asked for data and how the
//! reply is framed — format strings, the `\x1f` separator, byte-length
//! prefixes. tmux 3.8 emits the data bytes as-is. Model types receive a
//! [`RecordReader`] and read fields without knowing the framing.

pub(crate) mod field;
pub(crate) mod formats;
pub(crate) mod framing;
pub(crate) mod options;
pub(crate) mod record;

pub(crate) use framing::ByteParseError;
pub(crate) use record::{RecordReader, decode_all, decode_one};
