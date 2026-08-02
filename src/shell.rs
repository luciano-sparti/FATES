/// Split a shell-like command string into words, honouring single quotes,
/// double quotes, and backslash escapes. Does NOT perform environment or glob
/// expansion, so this is safe to feed directly to `Command`.
pub fn split(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_word = false;

    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c2) => current.push(c2),
                        None => break,
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => {
                            if let Some(&escaped) = chars.peek() {
                                current.push(escaped);
                                chars.next();
                            }
                        }
                        Some(c2) => current.push(c2),
                        None => break,
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(&escaped) = chars.peek() {
                    current.push(escaped);
                    chars.next();
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                current.push(c);
            }
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

/// Expand a leading `~` or `~/...` to the current user's home directory.
/// Non-tilde paths are returned unchanged.
pub fn expand_tilde(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    expand_tilde_with_home(path, &home)
}

/// Expand `$VAR` and `${VAR}` references using the process environment,
/// overlaid by `overlay` (group-level `env:` values take precedence). Unset
/// variables expand to the empty string. This runs after word-splitting, so
/// an expansion result is never re-split (no word splitting of expansions).
pub fn expand_env(input: &str, overlay: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('{') => {
                chars.next();
                let mut name = String::new();
                while let Some(&c2) = chars.peek() {
                    chars.next();
                    if c2 == '}' {
                        break;
                    }
                    name.push(c2);
                }
                out.push_str(&resolve(name, overlay));
            }
            Some(&c2) if is_var_start(c2) => {
                let mut name = String::new();
                while let Some(&c3) = chars.peek() {
                    if is_var_char(c3) {
                        name.push(c3);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push_str(&resolve(name, overlay));
            }
            _ => out.push('$'),
        }
    }
    out
}

fn is_var_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_var_char(c: char) -> bool {
    is_var_start(c) || c.is_ascii_digit()
}

fn resolve(name: String, overlay: &std::collections::HashMap<String, String>) -> String {
    if let Some(v) = overlay.get(&name) {
        return v.clone();
    }
    std::env::var(&name).unwrap_or_default()
}

fn expand_tilde_with_home(path: &str, home: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    if path == "~" {
        return home.to_string();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{}/{}", home, rest);
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_whitespace() {
        assert_eq!(split("echo hello world"), vec!["echo", "hello", "world"]);
    }

    #[test]
    fn collapses_leading_trailing_and_repeated_whitespace() {
        assert_eq!(split("  echo   hi  "), vec!["echo", "hi"]);
    }

    #[test]
    fn single_quotes_group_into_one_word() {
        assert_eq!(split("echo 'hello world'"), vec!["echo", "hello world"]);
        assert_eq!(split("echo 'it''s'"), vec!["echo", "its"]);
    }

    #[test]
    fn double_quotes_group_and_allow_escapes() {
        assert_eq!(split("echo \"hello world\""), vec!["echo", "hello world"]);
        assert_eq!(split("echo \"a\\\"b\""), vec!["echo", "a\"b"]);
    }

    #[test]
    fn backslash_escapes_next_character() {
        assert_eq!(split("echo a\\ b"), vec!["echo", "a b"]);
        assert_eq!(split("echo a\\\\b"), vec!["echo", "a\\b"]);
    }

    #[test]
    fn adjacent_quoted_and_unquoted_parts_merge() {
        assert_eq!(split("echo a\"b\"c"), vec!["echo", "abc"]);
    }

    #[test]
    fn empty_quoted_word_is_preserved() {
        assert_eq!(split("echo \"\""), vec!["echo", ""]);
    }

    #[test]
    fn empty_input_yields_no_words() {
        assert!(split("   ").is_empty());
    }

    #[test]
    fn tilde_expands_to_home() {
        let home = "/home/test";
        assert_eq!(expand_tilde_with_home("~", home), "/home/test");
        assert_eq!(expand_tilde_with_home("~/data", home), "/home/test/data");
    }

    #[test]
    fn non_tilde_paths_are_unchanged() {
        let home = "/home/test";
        assert_eq!(expand_tilde_with_home("/etc/hosts", home), "/etc/hosts");
        assert_eq!(expand_tilde_with_home("some~thing", home), "some~thing");
    }

    #[test]
    fn tilde_is_untouched_when_home_is_unknown() {
        assert_eq!(expand_tilde_with_home("~/data", ""), "~/data");
    }

    #[test]
    fn env_expands_simple_and_braced_forms() {
        let mut overlay = std::collections::HashMap::new();
        overlay.insert("DB_URL".to_string(), "postgres://localhost".to_string());

        assert_eq!(
            expand_env("run --db $DB_URL", &overlay),
            "run --db postgres://localhost"
        );
        assert_eq!(
            expand_env("run --db ${DB_URL}", &overlay),
            "run --db postgres://localhost"
        );
        assert_eq!(
            expand_env("run $DB_URL here", &overlay),
            "run postgres://localhost here"
        );
    }

    #[test]
    fn env_expands_from_process_environment() {
        let overlay = std::collections::HashMap::new();
        // HOME is set in the test environment; expansion should not crash on
        // unknown variables and should replace $HOME.
        let expanded = expand_env("echo $HOME", &overlay);
        assert_ne!(expanded, "echo $HOME", "HOME should have been expanded");
        assert!(!expanded.contains('$'));
    }

    #[test]
    fn env_unset_variables_expand_to_empty() {
        let overlay = std::collections::HashMap::new();
        assert_eq!(expand_env("a$NO_SUCH_VAR_XYZ b", &overlay), "a b");
        assert_eq!(expand_env("a${NO_SUCH_VAR_XYZ}b", &overlay), "ab");
    }

    #[test]
    fn env_overlay_takes_precedence() {
        let mut overlay = std::collections::HashMap::new();
        overlay.insert("HOME".to_string(), "/virtual/home".to_string());
        assert_eq!(expand_env("$HOME/x", &overlay), "/virtual/home/x");
    }

    #[test]
    fn env_lone_dollar_is_literal() {
        let overlay = std::collections::HashMap::new();
        assert_eq!(expand_env("cost $5", &overlay), "cost $5");
        assert_eq!(expand_env("$", &overlay), "$");
    }
}
