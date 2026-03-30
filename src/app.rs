use std::io::{self, Write};
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use clap::Parser;

use crate::backend::{Backend, OllamaBackend};
use crate::cli::{Cli, Command as CliCommand, ConfigCommand};
use crate::config::{
    CliOverrides, DEFAULT_OLLAMA_HOST, FileConfig, Mode, PREFERRED_OLLAMA_MODEL, Provider,
    ResolvedConfig, config_path, discover_default_model, env_config, load_file_config,
    parse_ollama_list, resolve_config, save_file_config,
};
use crate::error::{QuickcommandError, Result};
use crate::exec::{ExecutionResult, Executor, SystemExecutor};
use crate::model::{ClarificationTurn, GenerationRequest, ModelReply};
use crate::prompt::RuntimeContext;
use crate::safety::is_destructive;
use crate::shell_integration::{home_dir, install_zsh_integration, zshrc_path};

const MAX_CLARIFICATIONS: usize = 2;
const PROGRESS_MESSAGE: &str = "Generating response...";
const PROGRESS_DELAY: Duration = Duration::from_millis(120);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(80);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    EmitCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskResult {
    ExitCode(i32),
    EmittedCommand(String),
}

pub trait Ui {
    fn info(&mut self, message: &str) -> Result<()>;
    fn warn(&mut self, message: &str) -> Result<()>;
    fn start_progress(&mut self, message: &str) -> Result<()>;
    fn stop_progress(&mut self) -> Result<()>;
    fn ask_yes_no(&mut self, prompt: &str, default: bool) -> Result<bool>;
    fn ask_input(&mut self, prompt: &str, default: Option<&str>) -> Result<String>;
    fn ask_choice(
        &mut self,
        question: &str,
        options: &[String],
        recommended: Option<usize>,
    ) -> Result<String>;
}

#[derive(Debug)]
pub struct SystemUi {
    output_mode: OutputMode,
    progress: Option<ProgressState>,
}

#[derive(Debug)]
struct ProgressState {
    stop_tx: Sender<()>,
    join_handle: JoinHandle<io::Result<()>>,
}

impl SystemUi {
    pub fn new(output_mode: OutputMode) -> Self {
        Self {
            output_mode,
            progress: None,
        }
    }

    fn write_message(&self, message: &str, stderr_only: bool) {
        if stderr_only || self.output_mode == OutputMode::EmitCommand {
            eprintln!("{message}");
        } else {
            println!("{message}");
        }
    }

    fn print_prompt(&self, prompt: &str) -> Result<()> {
        if self.output_mode == OutputMode::EmitCommand {
            eprint!("{prompt}");
            io::stderr().flush()?;
        } else {
            print!("{prompt}");
            io::stdout().flush()?;
        }
        Ok(())
    }
}

impl Ui for SystemUi {
    fn info(&mut self, message: &str) -> Result<()> {
        self.write_message(message, false);
        Ok(())
    }

    fn warn(&mut self, message: &str) -> Result<()> {
        self.write_message(message, true);
        Ok(())
    }

    fn start_progress(&mut self, message: &str) -> Result<()> {
        self.stop_progress()?;
        let message = message.to_string();
        let (stop_tx, stop_rx) = mpsc::channel();
        let join_handle = thread::spawn(move || -> io::Result<()> {
            if stop_rx.recv_timeout(PROGRESS_DELAY).is_ok() {
                return Ok(());
            }

            let frames = ["|", "/", "-", "\\"];
            let mut frame_index = 0;

            loop {
                eprint!("\r\x1b[2K{} {}", frames[frame_index], message);
                io::stderr().flush()?;

                match stop_rx.recv_timeout(PROGRESS_INTERVAL) {
                    Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        frame_index = (frame_index + 1) % frames.len();
                    }
                }
            }

            eprint!("\r\x1b[2K");
            io::stderr().flush()?;
            Ok(())
        });

        self.progress = Some(ProgressState {
            stop_tx,
            join_handle,
        });
        Ok(())
    }

    fn stop_progress(&mut self) -> Result<()> {
        if let Some(progress) = self.progress.take() {
            let _ = progress.stop_tx.send(());
            match progress.join_handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(QuickcommandError::Io(io::Error::other(
                        "progress thread panicked",
                    )));
                }
            }
        }
        Ok(())
    }

    fn ask_yes_no(&mut self, prompt: &str, default: bool) -> Result<bool> {
        let default_suffix = if default { "[Y/n]" } else { "[y/N]" };
        loop {
            self.print_prompt(&format!("{prompt} {default_suffix}: "))?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let trimmed = input.trim();

            if trimmed.is_empty() {
                return Ok(default);
            }

            match trimmed.to_ascii_lowercase().as_str() {
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => self.warn("Please answer with y/yes or n/no.")?,
            }
        }
    }

    fn ask_input(&mut self, prompt: &str, default: Option<&str>) -> Result<String> {
        loop {
            match default {
                Some(value) => self.print_prompt(&format!("{prompt} [{value}]: "))?,
                None => self.print_prompt(&format!("{prompt}: "))?,
            }

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let trimmed = input.trim();

            if trimmed.is_empty() {
                if let Some(value) = default {
                    return Ok(value.to_string());
                }
            } else {
                return Ok(trimmed.to_string());
            }

            self.warn("Please enter a value.")?;
        }
    }

    fn ask_choice(
        &mut self,
        question: &str,
        options: &[String],
        recommended: Option<usize>,
    ) -> Result<String> {
        self.info(question)?;
        for (index, option) in options.iter().enumerate() {
            self.info(&format!("  {}. {}", index + 1, option))?;
        }

        loop {
            let default_label = recommended
                .and_then(|index| options.get(index))
                .map(|option| option.as_str());

            let answer =
                self.ask_input("Choose a number or type your own answer", default_label)?;
            if let Ok(number) = answer.parse::<usize>() {
                if let Some(option) = options.get(number.saturating_sub(1)) {
                    return Ok(option.clone());
                }
            }

            if !answer.trim().is_empty() {
                return Ok(answer);
            }

            self.warn("Please enter a valid number or value.")?;
        }
    }
}

pub fn run() -> Result<i32> {
    let cli = Cli::parse();
    let output_mode = if cli.emit_command {
        OutputMode::EmitCommand
    } else {
        OutputMode::Human
    };
    let mut ui = SystemUi::new(output_mode);
    run_cli(cli, output_mode, &mut ui)
}

fn run_cli(cli: Cli, output_mode: OutputMode, ui: &mut impl Ui) -> Result<i32> {
    match cli.command {
        Some(CliCommand::Init) => run_init(ui),
        Some(CliCommand::Config {
            command: ConfigCommand::Show,
        }) => run_config_show(ui),
        None => run_task_from_cli(cli, output_mode, ui),
    }
}

fn run_task_from_cli(cli: Cli, output_mode: OutputMode, ui: &mut impl Ui) -> Result<i32> {
    let task = cli.task_string().ok_or(QuickcommandError::MissingTask)?;
    let default_model = discover_default_model();
    let file_path = config_path()?;
    let file_config = load_file_config(&file_path)?;
    let env_config = env_config()?;
    let cli_overrides = CliOverrides {
        mode: if cli.emit_command {
            Some(Mode::Copy)
        } else {
            cli.mode_override()
        },
    };
    let resolved = resolve_config(
        file_config.as_ref(),
        &env_config,
        &cli_overrides,
        &default_model,
    );
    let context = RuntimeContext::detect()?;
    let backend = OllamaBackend::new(resolved.ollama_host.clone(), resolved.ollama_model.clone())?;
    let mut executor = SystemExecutor;

    match run_task_with_deps(
        &backend,
        ui,
        &mut executor,
        &task,
        &resolved,
        &context,
        output_mode,
    )? {
        TaskResult::ExitCode(code) => Ok(code),
        TaskResult::EmittedCommand(command) => {
            print!("{command}");
            io::stdout().flush()?;
            Ok(0)
        }
    }
}

fn run_init(ui: &mut impl Ui) -> Result<i32> {
    ui.info("Starting quickcommand setup.")?;

    let file_path = config_path()?;
    let default_model = discover_default_model();
    let host = ui.ask_input("Ollama host URL", Some(DEFAULT_OLLAMA_HOST))?;
    let installed_models = discover_installed_models();

    let model = if installed_models.is_empty() {
        ui.ask_input("Ollama model name", Some(&default_model))?
    } else {
        let mut options = installed_models.clone();
        options.push("Enter manually".into());
        let recommended = installed_models
            .iter()
            .position(|model| model == &default_model)
            .or(Some(0));
        let selected = ui.ask_choice("Select an Ollama model.", &options, recommended)?;
        if selected == "Enter manually" {
            ui.ask_input("Ollama model name", Some(&default_model))?
        } else {
            selected
        }
    };

    let file_config = FileConfig {
        provider: Some(Provider::Ollama),
        mode: Some(Mode::Copy),
        ollama_host: Some(host),
        ollama_model: Some(model),
    };
    save_file_config(&file_path, &file_config)?;

    let zshrc = zshrc_path(&home_dir()?);
    let installed = install_zsh_integration(&zshrc, "qc")?;

    ui.info(&format!("Saved configuration to {}.", file_path.display()))?;
    if installed {
        ui.info(&format!(
            "Installed zsh integration into {}.",
            zshrc.display()
        ))?;
    } else {
        ui.info(&format!(
            "zsh integration is already up to date in {}.",
            zshrc.display()
        ))?;
    }
    ui.info("Restart zsh or run `source ~/.zshrc` to enable prompt insertion.")?;
    Ok(0)
}

fn run_config_show(ui: &mut impl Ui) -> Result<i32> {
    let file_path = config_path()?;
    let default_model = discover_default_model();
    let file_config = load_file_config(&file_path)?;
    let env_config = env_config()?;
    let resolved = resolve_config(
        file_config.as_ref(),
        &env_config,
        &CliOverrides::default(),
        &default_model,
    );

    ui.info("quickcommand configuration")?;
    ui.info(&format!("  path         = {}", file_path.display()))?;
    ui.info(&format!("  provider     = {}", resolved.provider))?;
    ui.info(&format!("  mode         = {}", resolved.mode))?;
    ui.info(&format!("  ollama_host  = {}", resolved.ollama_host))?;
    ui.info(&format!("  ollama_model = {}", resolved.ollama_model))?;

    Ok(0)
}

fn discover_installed_models() -> Vec<String> {
    let output = Command::new("ollama").arg("list").output();
    match output {
        Ok(result) if result.status.success() => {
            parse_ollama_list(&String::from_utf8_lossy(&result.stdout))
        }
        _ => vec![PREFERRED_OLLAMA_MODEL.to_string()],
    }
}

pub fn run_task_with_deps<B: Backend, U: Ui, E: Executor>(
    backend: &B,
    ui: &mut U,
    executor: &mut E,
    task: &str,
    config: &ResolvedConfig,
    context: &RuntimeContext,
    output_mode: OutputMode,
) -> Result<TaskResult> {
    let mut clarification_history = Vec::new();

    loop {
        if clarification_history.len() > MAX_CLARIFICATIONS {
            return Err(QuickcommandError::ClarificationLimitReached);
        }

        let request = GenerationRequest {
            task: task.to_string(),
            clarification_history: clarification_history.clone(),
        };

        match generate_with_progress(backend, ui, &request, context)? {
            ModelReply::Command(reply) => match config.mode {
                Mode::Copy => match output_mode {
                    OutputMode::EmitCommand => {
                        return Ok(TaskResult::EmittedCommand(reply.command));
                    }
                    OutputMode::Human => {
                        ui.info(&format!("Summary: {}", reply.summary))?;
                        ui.info("Command:")?;
                        for line in reply.command.lines().filter(|line| !line.trim().is_empty()) {
                            ui.info(&format!("  $ {line}"))?;
                        }
                        return Ok(TaskResult::ExitCode(0));
                    }
                },
                Mode::Execute => {
                    ui.info(&format!("Summary: {}", reply.summary))?;
                    ui.info("Command:")?;
                    for line in reply.command.lines().filter(|line| !line.trim().is_empty()) {
                        ui.info(&format!("  $ {line}"))?;
                    }

                    if is_destructive(&reply.command)
                        && !ui
                            .ask_yes_no("Warning: this command looks destructive. Run it?", false)?
                    {
                        ui.warn("Execution cancelled.")?;
                        return Ok(TaskResult::ExitCode(130));
                    }

                    let ExecutionResult { exit_code } =
                        executor.execute(&context.shell, &reply.command)?;
                    return Ok(TaskResult::ExitCode(exit_code));
                }
            },
            ModelReply::Clarification(reply) => {
                if clarification_history.len() >= MAX_CLARIFICATIONS {
                    return Err(QuickcommandError::ClarificationLimitReached);
                }

                let answer =
                    ui.ask_choice(&reply.question, &reply.options, reply.recommended_index)?;
                clarification_history.push(ClarificationTurn {
                    question: reply.question,
                    answer,
                });
            }
        }
    }
}

fn generate_with_progress<B: Backend, U: Ui>(
    backend: &B,
    ui: &mut U,
    request: &GenerationRequest,
    context: &RuntimeContext,
) -> Result<ModelReply> {
    ui.start_progress(PROGRESS_MESSAGE)?;
    let generate_result = backend.generate(request, context);
    let stop_result = ui.stop_progress();

    match (generate_result, stop_result) {
        (Ok(reply), Ok(())) => Ok(reply),
        (Ok(_), Err(stop_err)) => Err(stop_err),
        (Err(generate_err), Ok(())) => Err(generate_err),
        (Err(generate_err), Err(_stop_err)) => Err(generate_err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Provider, ResolvedConfig};
    use crate::exec::Executor;
    use crate::model::{ClarificationReply, CommandReply, ModelReply};

    #[derive(Default)]
    struct FakeExecutor {
        commands: Vec<(String, String)>,
        exit_code: i32,
    }

    impl Executor for FakeExecutor {
        fn execute(&mut self, shell: &str, command: &str) -> Result<ExecutionResult> {
            self.commands.push((shell.to_string(), command.to_string()));
            Ok(ExecutionResult {
                exit_code: self.exit_code,
            })
        }
    }

    #[derive(Default)]
    struct FakeUi {
        messages: Vec<String>,
        answers: Vec<String>,
        confirm: bool,
        progress_events: Vec<String>,
    }

    impl Ui for FakeUi {
        fn info(&mut self, message: &str) -> Result<()> {
            self.messages.push(message.to_string());
            Ok(())
        }

        fn warn(&mut self, message: &str) -> Result<()> {
            self.messages.push(message.to_string());
            Ok(())
        }

        fn start_progress(&mut self, message: &str) -> Result<()> {
            self.progress_events.push(format!("start:{message}"));
            Ok(())
        }

        fn stop_progress(&mut self) -> Result<()> {
            self.progress_events.push("stop".to_string());
            Ok(())
        }

        fn ask_yes_no(&mut self, _prompt: &str, _default: bool) -> Result<bool> {
            Ok(self.confirm)
        }

        fn ask_input(&mut self, _prompt: &str, default: Option<&str>) -> Result<String> {
            Ok(default.unwrap_or_default().to_string())
        }

        fn ask_choice(
            &mut self,
            _question: &str,
            options: &[String],
            recommended: Option<usize>,
        ) -> Result<String> {
            if let Some(answer) = self.answers.first().cloned() {
                self.answers.remove(0);
                return Ok(answer);
            }
            if let Some(index) = recommended.and_then(|index| options.get(index)) {
                return Ok(index.clone());
            }
            Ok(options.first().cloned().unwrap_or_default())
        }
    }

    struct FakeBackend {
        replies: std::cell::RefCell<Vec<Result<ModelReply>>>,
    }

    impl Backend for FakeBackend {
        fn generate(
            &self,
            _request: &GenerationRequest,
            _context: &RuntimeContext,
        ) -> Result<ModelReply> {
            self.replies.borrow_mut().remove(0)
        }
    }

    fn context() -> RuntimeContext {
        RuntimeContext {
            os: "macos".into(),
            shell: "/bin/zsh".into(),
            cwd: "/tmp".into(),
            home: Some("/Users/test".into()),
        }
    }

    fn copy_config() -> ResolvedConfig {
        ResolvedConfig {
            provider: Provider::Ollama,
            mode: Mode::Copy,
            ollama_host: "http://localhost:11434".into(),
            ollama_model: "qwen3.5:9b".into(),
        }
    }

    #[test]
    fn human_copy_mode_prints_command_without_executing() {
        let backend = FakeBackend {
            replies: std::cell::RefCell::new(vec![Ok(ModelReply::Command(CommandReply {
                summary: "Show the current directory.".into(),
                command: "pwd".into(),
            }))]),
        };
        let mut ui = FakeUi::default();
        let mut executor = FakeExecutor::default();

        let result = run_task_with_deps(
            &backend,
            &mut ui,
            &mut executor,
            "show pwd",
            &copy_config(),
            &context(),
            OutputMode::Human,
        )
        .expect("copy flow should succeed");

        assert_eq!(result, TaskResult::ExitCode(0));
        assert!(
            ui.messages
                .iter()
                .any(|message| message == "Summary: Show the current directory.")
        );
        assert!(executor.commands.is_empty());
    }

    #[test]
    fn emit_command_mode_returns_raw_command_only() {
        let backend = FakeBackend {
            replies: std::cell::RefCell::new(vec![Ok(ModelReply::Command(CommandReply {
                summary: "Show the current directory.".into(),
                command: "pwd".into(),
            }))]),
        };
        let mut ui = FakeUi::default();
        let mut executor = FakeExecutor::default();

        let result = run_task_with_deps(
            &backend,
            &mut ui,
            &mut executor,
            "show pwd",
            &copy_config(),
            &context(),
            OutputMode::EmitCommand,
        )
        .expect("emit flow should succeed");

        assert_eq!(result, TaskResult::EmittedCommand("pwd".into()));
        assert!(ui.messages.is_empty());
        assert!(executor.commands.is_empty());
    }

    #[test]
    fn clarification_then_emit_command_works() {
        let backend = FakeBackend {
            replies: std::cell::RefCell::new(vec![
                Ok(ModelReply::Clarification(ClarificationReply {
                    question: "Which port should I check?".into(),
                    options: vec!["3000".into(), "8080".into()],
                    recommended_index: Some(0),
                })),
                Ok(ModelReply::Command(CommandReply {
                    summary: "Check port 8080.".into(),
                    command: "lsof -i :8080".into(),
                })),
            ]),
        };
        let mut ui = FakeUi {
            answers: vec!["8080".into()],
            ..Default::default()
        };
        let mut executor = FakeExecutor::default();

        let result = run_task_with_deps(
            &backend,
            &mut ui,
            &mut executor,
            "find the port owner",
            &copy_config(),
            &context(),
            OutputMode::EmitCommand,
        )
        .expect("clarification flow should succeed");

        assert_eq!(result, TaskResult::EmittedCommand("lsof -i :8080".into()));
    }

    #[test]
    fn execute_mode_respects_confirmation_for_destructive_commands() {
        let backend = FakeBackend {
            replies: std::cell::RefCell::new(vec![Ok(ModelReply::Command(CommandReply {
                summary: "Delete the temp file.".into(),
                command: "rm -rf /tmp/example".into(),
            }))]),
        };
        let mut ui = FakeUi {
            confirm: false,
            ..Default::default()
        };
        let mut executor = FakeExecutor::default();
        let mut config = copy_config();
        config.mode = Mode::Execute;

        let result = run_task_with_deps(
            &backend,
            &mut ui,
            &mut executor,
            "delete temp file",
            &config,
            &context(),
            OutputMode::Human,
        )
        .expect("decline should still be a clean exit");

        assert_eq!(result, TaskResult::ExitCode(130));
        assert!(executor.commands.is_empty());
    }

    #[test]
    fn command_generation_wraps_backend_call_with_progress() {
        let backend = FakeBackend {
            replies: std::cell::RefCell::new(vec![Ok(ModelReply::Command(CommandReply {
                summary: "Show the current directory.".into(),
                command: "pwd".into(),
            }))]),
        };
        let mut ui = FakeUi::default();
        let mut executor = FakeExecutor::default();

        run_task_with_deps(
            &backend,
            &mut ui,
            &mut executor,
            "show pwd",
            &copy_config(),
            &context(),
            OutputMode::Human,
        )
        .expect("copy flow should succeed");

        assert_eq!(
            ui.progress_events,
            vec![
                "start:Generating response...".to_string(),
                "stop".to_string()
            ]
        );
    }

    #[test]
    fn clarification_flow_starts_progress_for_each_backend_call() {
        let backend = FakeBackend {
            replies: std::cell::RefCell::new(vec![
                Ok(ModelReply::Clarification(ClarificationReply {
                    question: "Which port should I check?".into(),
                    options: vec!["3000".into(), "8080".into()],
                    recommended_index: Some(0),
                })),
                Ok(ModelReply::Command(CommandReply {
                    summary: "Check port 8080.".into(),
                    command: "lsof -i :8080".into(),
                })),
            ]),
        };
        let mut ui = FakeUi {
            answers: vec!["8080".into()],
            ..Default::default()
        };
        let mut executor = FakeExecutor::default();

        run_task_with_deps(
            &backend,
            &mut ui,
            &mut executor,
            "find the port owner",
            &copy_config(),
            &context(),
            OutputMode::EmitCommand,
        )
        .expect("clarification flow should succeed");

        assert_eq!(
            ui.progress_events,
            vec![
                "start:Generating response...".to_string(),
                "stop".to_string(),
                "start:Generating response...".to_string(),
                "stop".to_string()
            ]
        );
    }

    #[test]
    fn backend_error_still_stops_progress() {
        let backend = FakeBackend {
            replies: std::cell::RefCell::new(vec![Err(QuickcommandError::OllamaApi(
                "boom".into(),
            ))]),
        };
        let mut ui = FakeUi::default();
        let mut executor = FakeExecutor::default();

        let error = run_task_with_deps(
            &backend,
            &mut ui,
            &mut executor,
            "show pwd",
            &copy_config(),
            &context(),
            OutputMode::Human,
        )
        .expect_err("backend error should bubble up");

        assert!(matches!(error, QuickcommandError::OllamaApi(_)));
        assert_eq!(
            ui.progress_events,
            vec![
                "start:Generating response...".to_string(),
                "stop".to_string()
            ]
        );
    }
}
