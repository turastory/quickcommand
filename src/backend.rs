use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;

use crate::error::{QuickcommandError, Result};
use crate::model::{GenerationRequest, ModelReply, parse_model_reply, reply_schema};
use crate::prompt::{RuntimeContext, build_system_prompt, build_user_prompt};

pub trait Backend {
    fn generate(&self, request: &GenerationRequest, context: &RuntimeContext)
    -> Result<ModelReply>;
}

#[derive(Debug, Clone)]
pub struct OllamaBackend {
    host: String,
    model: String,
    client: Client,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    content: String,
}

impl OllamaBackend {
    pub fn new(host: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        let client = Client::builder().timeout(Duration::from_secs(60)).build()?;

        Ok(Self {
            host: host.into().trim_end_matches('/').to_string(),
            model: model.into(),
            client,
        })
    }
}

impl Backend for OllamaBackend {
    fn generate(
        &self,
        request: &GenerationRequest,
        context: &RuntimeContext,
    ) -> Result<ModelReply> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "format": reply_schema(),
            "options": {
                "temperature": 0
            },
            "messages": [
                {
                    "role": "system",
                    "content": build_system_prompt(context)
                },
                {
                    "role": "user",
                    "content": build_user_prompt(request)
                }
            ]
        });

        let response = self
            .client
            .post(format!("{}/api/chat", self.host))
            .json(&body)
            .send()?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(QuickcommandError::OllamaApi(format!(
                "status={} body={}",
                status, body
            )));
        }

        let response: OllamaResponse = response.json()?;
        parse_model_reply(&response.message.content)
    }
}

#[cfg(test)]
mod tests {
    use mockito::Server;
    use serde_json::json;

    use super::*;
    use crate::model::{CommandReply, ModelReply};

    fn context() -> RuntimeContext {
        RuntimeContext {
            os: "macos".into(),
            shell: "/bin/zsh".into(),
            cwd: "/tmp".into(),
            home: Some("/Users/test".into()),
        }
    }

    fn request() -> GenerationRequest {
        GenerationRequest {
            task: "현재 디렉터리 보여줘".into(),
            clarification_history: vec![],
        }
    }

    #[test]
    fn backend_parses_command_response_from_mock_server() {
        let mut server = Server::new();
        let body = json!({
            "message": {
                "content": "{\"response_type\":\"command\",\"summary\":\"현재 경로를 출력합니다.\",\"command\":\"pwd\"}"
            }
        });

        let _mock = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create();

        let backend = OllamaBackend::new(server.url(), "qwen3.5:9b").expect("backend should build");
        let reply = backend
            .generate(&request(), &context())
            .expect("response should parse");

        assert_eq!(
            reply,
            ModelReply::Command(CommandReply {
                summary: "현재 경로를 출력합니다.".into(),
                command: "pwd".into(),
            })
        );
    }

    #[test]
    fn backend_fails_on_malformed_model_reply() {
        let mut server = Server::new();
        let body = json!({
            "message": {
                "content": "{\"response_type\":\"command\",\"summary\":\"요약만 있음\"}"
            }
        });

        let _mock = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create();

        let backend = OllamaBackend::new(server.url(), "qwen3.5:9b").expect("backend should build");
        let error = backend
            .generate(&request(), &context())
            .expect_err("malformed response should fail");

        assert!(matches!(error, QuickcommandError::InvalidModelReply(_)));
    }
}
