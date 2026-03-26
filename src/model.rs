use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{QuickcommandError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClarificationTurn {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationRequest {
    pub task: String,
    pub clarification_history: Vec<ClarificationTurn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReply {
    pub summary: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClarificationReply {
    pub question: String,
    pub options: Vec<String>,
    pub recommended_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelReply {
    Command(CommandReply),
    Clarification(ClarificationReply),
}

#[derive(Debug, Deserialize)]
struct RawReply {
    response_type: String,
    summary: Option<String>,
    command: Option<String>,
    question: Option<String>,
    options: Option<Vec<String>>,
    recommended_index: Option<usize>,
}

pub fn reply_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "response_type": {
                "type": "string",
                "enum": ["command", "clarification"]
            },
            "summary": { "type": "string" },
            "command": { "type": "string" },
            "question": { "type": "string" },
            "options": {
                "type": "array",
                "items": { "type": "string" }
            },
            "recommended_index": { "type": "integer", "minimum": 0 }
        },
        "required": ["response_type"],
        "additionalProperties": false
    })
}

pub fn parse_model_reply(content: &str) -> Result<ModelReply> {
    let parsed: RawReply = serde_json::from_str(content)
        .map_err(|err| QuickcommandError::InvalidModelReply(err.to_string()))?;

    match parsed.response_type.as_str() {
        "command" => {
            let summary = parsed
                .summary
                .unwrap_or_else(|| "Generated command.".to_string());
            let command = parsed.command.ok_or_else(|| {
                QuickcommandError::InvalidModelReply("missing `command` field".into())
            })?;
            Ok(ModelReply::Command(CommandReply { summary, command }))
        }
        "clarification" => {
            let question = parsed.question.ok_or_else(|| {
                QuickcommandError::InvalidModelReply("missing `question` field".into())
            })?;
            let options = parsed.options.ok_or_else(|| {
                QuickcommandError::InvalidModelReply("missing `options` field".into())
            })?;
            if options.is_empty() {
                return Err(QuickcommandError::InvalidModelReply(
                    "`options` must contain at least one item".into(),
                ));
            }
            Ok(ModelReply::Clarification(ClarificationReply {
                question,
                options,
                recommended_index: parsed.recommended_index,
            }))
        }
        other => Err(QuickcommandError::InvalidModelReply(format!(
            "unknown response_type: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_reply() {
        let reply = parse_model_reply(
            r#"{"response_type":"command","summary":"Print the current directory.","command":"pwd"}"#,
        )
        .expect("command reply should parse");

        assert_eq!(
            reply,
            ModelReply::Command(CommandReply {
                summary: "Print the current directory.".into(),
                command: "pwd".into(),
            })
        );
    }

    #[test]
    fn parses_clarification_reply() {
        let reply = parse_model_reply(
            r#"{"response_type":"clarification","question":"Which port should I check?","options":["3000","8080"],"recommended_index":0}"#,
        )
        .expect("clarification reply should parse");

        assert_eq!(
            reply,
            ModelReply::Clarification(ClarificationReply {
                question: "Which port should I check?".into(),
                options: vec!["3000".into(), "8080".into()],
                recommended_index: Some(0),
            })
        );
    }

    #[test]
    fn command_reply_without_summary_uses_fallback() {
        let reply = parse_model_reply(r#"{"response_type":"command","command":"pwd"}"#)
            .expect("command reply without summary should still parse");

        assert_eq!(
            reply,
            ModelReply::Command(CommandReply {
                summary: "Generated command.".into(),
                command: "pwd".into(),
            })
        );
    }
}
