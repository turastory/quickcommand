use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum QuickcommandError {
    #[error("A task is required. Run `qc \"<task>\"`.")]
    MissingTask,
    #[error("Unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error("Invalid model reply: {0}")]
    InvalidModelReply(String),
    #[error("Too many clarification rounds.")]
    ClarificationLimitReached,
    #[error("Execution was cancelled by the user.")]
    UserDeclined,
    #[error("Ollama request failed: {0}")]
    OllamaApi(String),
    #[error("Failed to parse TOML: {0}")]
    TomlDeserialize(#[from] toml::de::Error),
    #[error("Failed to serialize TOML: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, QuickcommandError>;
