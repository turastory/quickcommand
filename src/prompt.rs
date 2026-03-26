use std::env;
use std::path::PathBuf;

use crate::error::Result;
use crate::model::GenerationRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContext {
    pub os: String,
    pub shell: String,
    pub cwd: PathBuf,
    pub home: Option<PathBuf>,
}

impl RuntimeContext {
    pub fn detect() -> Result<Self> {
        Ok(Self {
            os: env::consts::OS.to_string(),
            shell: env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            cwd: env::current_dir()?,
            home: env::var_os("HOME").map(PathBuf::from),
        })
    }
}

pub fn build_system_prompt(context: &RuntimeContext) -> String {
    format!(
        "\
You are `quickcommand` (`qc`), a local-first CLI assistant that converts natural language tasks into a single paste-ready shell command or multi-line shell snippet.

You must return JSON that matches the provided schema exactly.

Rules:
- Output only one of two response types: `command` or `clarification`.
- Prefer the minimum sufficient command.
- Avoid destructive or forceful commands unless clearly required.
- Do not use tool calls.
- Do not explain shell output you have not observed.
- Return a complete command the user can paste directly.
- Keep the summary short.
- Match the user's language.
- Target this environment:
  - OS: {os}
  - Shell: {shell}
  - Working directory: {cwd}
  - Home directory: {home}",
        os = context.os,
        shell = context.shell,
        cwd = context.cwd.display(),
        home = context
            .home
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "(unknown)".to_string()),
    )
}

pub fn build_user_prompt(request: &GenerationRequest) -> String {
    let mut prompt = format!("User task:\n{}\n", request.task);

    if !request.clarification_history.is_empty() {
        prompt.push_str("\nResolved clarifications:\n");
        for turn in &request.clarification_history {
            prompt.push_str(&format!(
                "- Question: {}\n- Answer: {}\n",
                turn.question, turn.answer
            ));
        }
    }

    prompt.push_str(
        "\nIf you still need one critical detail, return `clarification`. Otherwise return `command`.",
    );

    prompt
}
