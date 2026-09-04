//! Parsing the output of `tmux show-options`.

use std::collections::HashMap;

/// Parse `tmux show-options` output into a map.
///
/// Lines without a space (bare flags) are skipped. Values that are empty or
/// equal to `''` are filtered out.
pub(crate) fn parse_options(buffer: &str) -> HashMap<String, String> {
    buffer
        .trim_end()
        .split('\n')
        .filter_map(|s| s.split_once(' '))
        .map(|(k, v)| (k, v.trim_start()))
        .filter(|(_, v)| !v.is_empty() && v != &"''")
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_options;

    #[test]
    fn parse_options_typical_output() {
        let input = "default-shell /bin/zsh\nstatus on\nhistory-limit 10000\n";
        let opts = parse_options(input);

        assert_eq!(opts.get("default-shell").unwrap(), "/bin/zsh");
        assert_eq!(opts.get("status").unwrap(), "on");
        assert_eq!(opts.get("history-limit").unwrap(), "10000");
    }

    #[test]
    fn parse_options_skips_bare_flags() {
        let input = "destroy-unattached\ndefault-shell /bin/zsh\nsilence-action\n";
        let opts = parse_options(input);

        assert_eq!(opts.len(), 1);
        assert_eq!(opts.get("default-shell").unwrap(), "/bin/zsh");
        assert!(!opts.contains_key("destroy-unattached"));
        assert!(!opts.contains_key("silence-action"));
    }

    #[test]
    fn parse_options_filters_empty_values() {
        let input = "default-command ''\ndefault-shell /bin/zsh\n";
        let opts = parse_options(input);

        assert!(!opts.contains_key("default-command"));
        assert_eq!(opts.get("default-shell").unwrap(), "/bin/zsh");
    }

    #[test]
    fn parse_options_empty_input() {
        let opts = parse_options("");
        assert!(opts.is_empty());
    }

    #[test]
    fn parse_options_value_with_spaces() {
        let input = "status-left [#S] #H\nstatus on\n";
        let opts = parse_options(input);

        assert_eq!(opts.get("status-left").unwrap(), "[#S] #H");
        assert_eq!(opts.get("status").unwrap(), "on");
    }

    #[test]
    fn parse_options_trims_spaces_between_key_and_value() {
        let input = "key   value-with-extra-spaces\n";
        let opts = parse_options(input);

        assert_eq!(opts.get("key").unwrap(), "value-with-extra-spaces");
    }
}
