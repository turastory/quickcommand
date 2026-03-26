use clap::{Parser, Subcommand};

use crate::config::Mode;

#[derive(Debug, Parser)]
#[command(
    name = "qc",
    version,
    about = "Local-first natural language shell assistant"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(long, help = "Execute the generated command instead of copying it.")]
    pub execute: bool,

    #[arg(long, hide = true, conflicts_with = "execute")]
    pub emit_command: bool,

    #[arg(value_name = "TASK", trailing_var_arg = true)]
    pub task: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Show,
}

impl Cli {
    pub fn task_string(&self) -> Option<String> {
        if self.task.is_empty() {
            None
        } else {
            Some(self.task.join(" "))
        }
    }

    pub fn mode_override(&self) -> Option<Mode> {
        self.execute.then_some(Mode::Execute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn default_mode_is_copy() {
        let cli = Cli::parse_from(["qc", "pwd"]);
        assert_eq!(cli.mode_override(), None);
    }

    #[test]
    fn execute_flag_overrides_mode() {
        let cli = Cli::parse_from(["qc", "--execute", "pwd"]);
        assert_eq!(cli.mode_override(), Some(Mode::Execute));
    }

    #[test]
    fn removed_model_flag_is_rejected() {
        let parsed = Cli::try_parse_from(["qc", "--model", "qwen3.5:9b", "pwd"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn removed_confirm_flag_is_rejected() {
        let parsed = Cli::try_parse_from(["qc", "--confirm", "none", "pwd"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn hidden_emit_command_flag_is_parsed() {
        let cli = Cli::parse_from(["qc", "--emit-command", "pwd"]);
        assert!(cli.emit_command);
    }
}
