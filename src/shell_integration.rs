use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{QuickcommandError, Result};

pub const ZSH_MARKER_START: &str = "# >>> quickcommand zsh integration >>>";
pub const ZSH_MARKER_END: &str = "# <<< quickcommand zsh integration <<<";

pub fn zshrc_path(home: &Path) -> PathBuf {
    home.join(".zshrc")
}

pub fn render_zsh_integration(binary_name: &str) -> String {
    format!(
        "{start}
qc() {{
  if [[ $# -eq 0 ]]; then
    command {binary_name} \"$@\"
    return $?
  fi

  case \"$1\" in
    --execute|--emit-command|--help|-h|--version|-v|init|config)
      command {binary_name} \"$@\"
      return $?
      ;;
  esac

  local cmd
  cmd=\"$(command {binary_name} --emit-command \"$@\")\" || return $?
  if [[ -n \"$cmd\" ]]; then
    print -z -- \"$cmd\"
  fi
}}
{end}
",
        start = ZSH_MARKER_START,
        end = ZSH_MARKER_END,
        binary_name = binary_name,
    )
}

pub fn upsert_managed_block(existing: &str, block: &str) -> String {
    match (
        existing.find(ZSH_MARKER_START),
        existing.find(ZSH_MARKER_END),
    ) {
        (Some(start), Some(end)) if start <= end => {
            let end_inclusive = end + ZSH_MARKER_END.len();
            let mut updated = String::with_capacity(existing.len() + block.len());
            updated.push_str(&existing[..start]);
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(block.trim_end());
            updated.push('\n');
            let suffix = existing[end_inclusive..].trim_start_matches('\n');
            if !suffix.is_empty() {
                updated.push('\n');
                updated.push_str(suffix);
                if existing.ends_with('\n') && !updated.ends_with('\n') {
                    updated.push('\n');
                }
            }
            updated
        }
        _ => {
            if existing.trim().is_empty() {
                format!("{}\n", block.trim_end())
            } else {
                let trimmed = existing.trim_end();
                format!("{trimmed}\n\n{}\n", block.trim_end())
            }
        }
    }
}

pub fn install_zsh_integration(path: &Path, binary_name: &str) -> Result<bool> {
    let existing = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };

    let updated = upsert_managed_block(&existing, &render_zsh_integration(binary_name));
    if updated == existing {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, updated)?;
    Ok(true)
}

pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| QuickcommandError::Io(std::io::Error::other("HOME is not set.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_contains_emit_command_and_print_z() {
        let snippet = render_zsh_integration("qc");
        assert!(snippet.contains("command qc --emit-command"));
        assert!(snippet.contains("print -z -- \"$cmd\""));
    }

    #[test]
    fn upsert_is_idempotent() {
        let first = upsert_managed_block("export PATH=$PATH\n", &render_zsh_integration("qc"));
        let second = upsert_managed_block(&first, &render_zsh_integration("qc"));
        assert_eq!(first, second);
    }

    #[test]
    fn upsert_replaces_existing_block() {
        let original = format!(
            "export PATH=$PATH\n\n{}\nold\n{}\n",
            ZSH_MARKER_START, ZSH_MARKER_END
        );
        let updated = upsert_managed_block(&original, &render_zsh_integration("qc"));
        assert!(!updated.contains("\nold\n"));
        assert!(updated.contains("command qc --emit-command"));
    }
}
