use std::process::{Command, Stdio};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub exit_code: i32,
}

pub trait Executor {
    fn execute(&mut self, shell: &str, command: &str) -> Result<ExecutionResult>;
}

#[derive(Debug, Default)]
pub struct SystemExecutor;

impl Executor for SystemExecutor {
    fn execute(&mut self, shell: &str, command: &str) -> Result<ExecutionResult> {
        let status = Command::new(shell)
            .arg("-lc")
            .arg(command)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        Ok(ExecutionResult {
            exit_code: status.code().unwrap_or(1),
        })
    }
}
