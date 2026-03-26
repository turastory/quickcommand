const DESTRUCTIVE_PATTERNS: &[&str] = &[
    "kill ",
    "killall ",
    "pkill ",
    "rm ",
    "rm -",
    "rmdir ",
    "dd ",
    "mkfs",
    "shutdown",
    "reboot",
    "truncate ",
    "drop ",
    "> /dev/",
];

pub fn is_destructive(command: &str) -> bool {
    let lowered = command.to_ascii_lowercase();
    DESTRUCTIVE_PATTERNS
        .iter()
        .any(|pattern| lowered.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_destructive_commands() {
        assert!(is_destructive("rm -rf /tmp/test"));
        assert!(is_destructive("kill 1234"));
        assert!(is_destructive("dd if=/dev/zero of=/tmp/out"));
        assert!(!is_destructive("ls -la"));
    }
}
