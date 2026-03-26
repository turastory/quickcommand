use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{QuickcommandError, Result};

pub const DEFAULT_OLLAMA_HOST: &str = "http://localhost:11434";
pub const PREFERRED_OLLAMA_MODEL: &str = "qwen3.5:9b";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Ollama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Copy,
    Execute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FileConfig {
    pub provider: Option<Provider>,
    pub mode: Option<Mode>,
    pub ollama_host: Option<String>,
    pub ollama_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    pub provider: Provider,
    pub mode: Mode,
    pub ollama_host: String,
    pub ollama_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvConfig {
    pub provider: Option<Provider>,
    pub mode: Option<Mode>,
    pub ollama_host: Option<String>,
    pub ollama_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliOverrides {
    pub mode: Option<Mode>,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::Ollama => write!(f, "ollama"),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mode::Copy => write!(f, "copy"),
            Mode::Execute => write!(f, "execute"),
        }
    }
}

impl Provider {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "ollama" => Ok(Self::Ollama),
            other => Err(QuickcommandError::UnsupportedProvider(other.to_string())),
        }
    }
}

impl Mode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "copy" => Some(Self::Copy),
            "execute" => Some(Self::Execute),
            _ => None,
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    config_path_from_env(
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
    )
}

pub fn config_path_from_env(xdg_config_home: Option<&str>, home: Option<&str>) -> Result<PathBuf> {
    if let Some(xdg) = xdg_config_home {
        return Ok(PathBuf::from(xdg).join("quickcommand").join("config.toml"));
    }

    if let Some(home) = home {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join("quickcommand")
            .join("config.toml"));
    }

    Err(QuickcommandError::Io(std::io::Error::other(
        "Could not determine the config directory.",
    )))
}

pub fn load_file_config(path: &Path) -> Result<Option<FileConfig>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;
    let config = toml::from_str::<FileConfig>(&content)?;
    Ok(Some(config))
}

pub fn save_file_config(path: &Path, config: &FileConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn env_config() -> Result<EnvConfig> {
    let provider = env::var("QC_PROVIDER")
        .ok()
        .map(|value| Provider::parse(&value))
        .transpose()?;

    Ok(EnvConfig {
        provider,
        mode: env::var("QC_MODE")
            .ok()
            .and_then(|value| Mode::parse(&value)),
        ollama_host: env::var("QC_OLLAMA_HOST")
            .ok()
            .or_else(|| env::var("OLLAMA_HOST").ok()),
        ollama_model: env::var("QC_OLLAMA_MODEL").ok(),
    })
}

pub fn resolve_config(
    file: Option<&FileConfig>,
    env: &EnvConfig,
    cli: &CliOverrides,
    default_model: &str,
) -> ResolvedConfig {
    ResolvedConfig {
        provider: cli_provider(env, file).unwrap_or(Provider::Ollama),
        mode: cli
            .mode
            .or(env.mode)
            .or(file.and_then(|cfg| cfg.mode))
            .unwrap_or_default(),
        ollama_host: env
            .ollama_host
            .clone()
            .or_else(|| file.and_then(|cfg| cfg.ollama_host.clone()))
            .unwrap_or_else(|| DEFAULT_OLLAMA_HOST.to_string()),
        ollama_model: env
            .ollama_model
            .clone()
            .or_else(|| file.and_then(|cfg| cfg.ollama_model.clone()))
            .unwrap_or_else(|| default_model.to_string()),
    }
}

fn cli_provider(env: &EnvConfig, file: Option<&FileConfig>) -> Option<Provider> {
    env.provider.or(file.and_then(|cfg| cfg.provider))
}

pub fn parse_ollama_list(output: &str) -> Vec<String> {
    output
        .lines()
        .skip_while(|line| line.trim().is_empty())
        .filter(|line| !line.trim_start().starts_with("NAME"))
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

pub fn pick_default_model(models: &[String]) -> Option<String> {
    if models.is_empty() {
        return None;
    }

    models
        .iter()
        .find(|model| model.as_str() == PREFERRED_OLLAMA_MODEL)
        .cloned()
        .or_else(|| models.first().cloned())
}

pub fn discover_default_model() -> String {
    let output = Command::new("ollama").arg("list").output();
    match output {
        Ok(result) if result.status.success() => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let models = parse_ollama_list(&stdout);
            pick_default_model(&models).unwrap_or_else(|| PREFERRED_OLLAMA_MODEL.to_string())
        }
        _ => PREFERRED_OLLAMA_MODEL.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_env_file_precedence_is_correct() {
        let file = FileConfig {
            provider: Some(Provider::Ollama),
            mode: Some(Mode::Copy),
            ollama_host: Some("http://file-host".into()),
            ollama_model: Some("file-model".into()),
        };
        let env = EnvConfig {
            provider: Some(Provider::Ollama),
            mode: Some(Mode::Execute),
            ollama_host: Some("http://env-host".into()),
            ollama_model: Some("env-model".into()),
        };
        let cli = CliOverrides {
            mode: Some(Mode::Copy),
        };

        let resolved = resolve_config(Some(&file), &env, &cli, "default-model");

        assert_eq!(resolved.mode, Mode::Copy);
        assert_eq!(resolved.ollama_host, "http://env-host");
        assert_eq!(resolved.ollama_model, "env-model");
    }

    #[test]
    fn preferred_model_is_selected_first() {
        let models = vec![
            "llama3.2:3b".to_string(),
            "qwen3.5:9b".to_string(),
            "starcoder2:3b".to_string(),
        ];

        let selected = pick_default_model(&models);

        assert_eq!(selected.as_deref(), Some("qwen3.5:9b"));
    }

    #[test]
    fn first_model_is_fallback_when_preferred_missing() {
        let models = vec!["llama3.2:3b".to_string(), "starcoder2:3b".to_string()];
        let selected = pick_default_model(&models);
        assert_eq!(selected.as_deref(), Some("llama3.2:3b"));
    }

    #[test]
    fn parse_ollama_list_reads_first_column() {
        let output = "\
NAME             ID              SIZE      MODIFIED
llama3.2:3b      abc             2.0 GB    6 hours ago
qwen3.5:9b       def             6.6 GB    11 days ago
";

        let parsed = parse_ollama_list(output);

        assert_eq!(parsed, vec!["llama3.2:3b", "qwen3.5:9b"]);
    }

    #[test]
    fn config_path_defaults_to_dot_config() {
        let path = config_path_from_env(None, Some("/Users/test")).expect("path should resolve");
        assert_eq!(
            path,
            PathBuf::from("/Users/test/.config/quickcommand/config.toml")
        );
    }
}
