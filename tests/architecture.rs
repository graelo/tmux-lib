//! Tests that the crate's layering holds.
//!
//! Two facts make the layering real rather than aspirational: tmux is started
//! in exactly one place, and the framed wire protocol is spelled in exactly
//! one module. Both are greppable, so both are checked here rather than left
//! to review. A second transport is coming; without these, adding it would
//! re-smear the protocol the way the first one did.
//!
//! This lives outside `src/` because the assertions themselves contain the
//! literals they forbid.

use std::fs;
use std::path::{Path, PathBuf};

/// Every `.rs` file under `src/`, as (repo-relative path, shipping contents).
///
/// Test modules are cut off at their `#[cfg(test)]` attribute. The invariants
/// below are about code that ships: a fixture asserting how a framed record
/// decodes has every reason to spell one out, whereas an operation building a
/// format string does not.
fn source_files() -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("failed to read a source directory") {
            let path = entry.expect("failed to read a directory entry").path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let contents = fs::read_to_string(&path).expect("failed to read a source file");
            let contents = strip_test_modules(&contents);
            let relative = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .expect("source files live under the manifest directory")
                .to_path_buf();
            (relative, contents)
        })
        .collect()
}

/// Remove every `#[cfg(test)]` item, keeping whatever follows it.
///
/// Newlines replace what is removed, so reported line numbers still match the
/// file on disk.
fn strip_test_modules(contents: &str) -> String {
    let mut kept = String::with_capacity(contents.len());
    let mut rest = contents;

    while let Some(offset) = rest.find("#[cfg(test)]") {
        kept.push_str(&rest[..offset]);
        let after = &rest[offset..];

        // Skip to the end of the attributed item by matching its braces.
        let Some(open) = after.find('{') else {
            break;
        };
        let mut depth = 0usize;
        let mut end = None;
        for (index, byte) in after.bytes().enumerate().skip(open) {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(index + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            break;
        };

        for _ in after[..end].bytes().filter(|byte| *byte == b'\n') {
            kept.push('\n');
        }
        rest = &after[end..];
    }
    kept.push_str(rest);

    kept
}

/// Report every `path:line` where `needle` occurs outside `allowed_prefix`.
fn offenders(needle: &str, allowed_prefix: &str) -> Vec<String> {
    source_files()
        .into_iter()
        .filter(|(path, _)| !path.starts_with(allowed_prefix))
        .flat_map(|(path, contents)| {
            contents
                .lines()
                .enumerate()
                .filter(|(_, line)| line.contains(needle))
                .map(|(index, line)| format!("{}:{}: {}", path.display(), index + 1, line.trim()))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn only_the_transport_starts_a_tmux_process() {
    let offenders = offenders("Command::new", "src/transport");

    assert!(
        offenders.is_empty(),
        "`Command::new` belongs only in src/transport, so that every invocation \
         shares one argument prefix. Found it in:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn only_the_wire_module_spells_the_framed_protocol() {
    // The framed record protocol, specifically: the separator byte, the byte
    // length prefix, and the backslash-doubling substitution. A plain
    // `#{pane_title}` in a shell example is a tmux format, not this crate's
    // framing, and is not what this guards.
    for needle in ["\\x1f", "#{n:", "#{s|"] {
        let offenders = offenders(needle, "src/wire");

        assert!(
            offenders.is_empty(),
            "`{needle}` is part of the framed protocol and belongs only in \
             src/wire. Found it in:\n  {}",
            offenders.join("\n  ")
        );
    }
}

#[test]
fn only_the_control_module_spells_the_line_protocol() {
    // The block markers, specifically. A second transport was the reason the
    // first two invariants exist; this is the one that keeps *its* protocol
    // from spreading the same way, now that operations are written against a
    // reply type rather than against either transport's wire format.
    for needle in ["%begin", "%end ", "%error ", "%exit"] {
        let offenders = offenders(needle, "src/transport/control");

        assert!(
            offenders.is_empty(),
            "`{needle}` is part of the control-mode line protocol and belongs \
             only in src/transport/control. Found it in:\n  {}",
            offenders.join("\n  ")
        );
    }
}

#[test]
fn the_handle_can_be_shared_across_threads() {
    fn assert_shareable<T: Send + Sync + Clone>() {}

    assert_shareable::<tmux_lib::Tmux>();
}
