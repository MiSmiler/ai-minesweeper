//! The DeepSeek Chat Completions client, its wire DTOs, and the `LlmClient`
//! abstraction the session drives against (a fake substitutes for it in
//! tests). The wire shape covers DeepSeek's extensions: `thinking`,
//! `reasoning_effort`, and `reasoning_content`.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// The harness configuration, read from the environment at startup.
#[derive(Debug, Clone)]
pub struct AiConfig {
    pub api_key: String,
    pub model: String,
}

impl AiConfig {
    /// Reads the config from the environment. `MY_DS_API_KEY` is required;
    /// `MY_DS_MODEL` defaults to `deepseek-v4-flash`. Returns `None` when no
    /// API key is set, which disables the AI routes with a clear error.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("MY_DS_API_KEY").ok()?;
        let model = std::env::var("MY_DS_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
        Some(Self { api_key, model })
    }
}

/// A message in the conversation. `reasoning_content` is display-only — the
/// API ignores it on the wire, so the session deliberately omits it when
/// re-sending an assistant turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

/// The (non-streaming) chat completion request body.
#[derive(Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<&'a [serde_json::Value]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<&'a str>,
    pub stream: bool,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Debug)]
pub enum LlmError {
    Http(reqwest::Error),
    /// A non-2xx status (e.g. 401 with the code/body for diagnosis).
    Status(u16, String),
    Empty,
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Http(e) => write!(f, "request error: {e}"),
            LlmError::Status(code, body) => write!(f, "api status {code}: {body}"),
            LlmError::Empty => write!(f, "empty response"),
        }
    }
}

/// The abstraction over the model: the session drives the loop against this,
/// and tests substitute a fake.
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, request: &ChatRequest<'_>) -> Result<ChatMessage, LlmError>;
}

/// The real DeepSeek client — a thin reqwest adapter to
/// `https://api.deepseek.com/chat/completions`.
pub struct DeepSeekClient {
    http: Client,
    config: AiConfig,
}

impl DeepSeekClient {
    pub fn new(config: AiConfig) -> Self {
        Self {
            http: Client::new(),
            config,
        }
    }
}

#[async_trait]
impl LlmClient for DeepSeekClient {
    async fn complete(&self, request: &ChatRequest<'_>) -> Result<ChatMessage, LlmError> {
        let resp = self
            .http
            .post("https://api.deepseek.com/chat/completions")
            .bearer_auth(&self.config.api_key)
            .json(request)
            .send()
            .await
            .map_err(LlmError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Status(status.as_u16(), body));
        }
        let body: ChatResponse = resp.json().await.map_err(LlmError::Http)?;
        body.choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or(LlmError::Empty)
    }
}
