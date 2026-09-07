//! The real DeepSeek provider (issue #116, ADR-0013) — the OpenAI-compatible
//! backend the runtime talks to over `POST /chat/completions`.
//!
//! The module owns every vendor-specific concern:
//! - an OpenAI-compatible HTTP/SSE client built on `reqwest`;
//! - the `content` / `reasoning_content` split from the SSE delta stream;
//! - the `image_url` / vision payload (via `ChatRequest`'s hand-written serde);
//! - an error-code mapping ([`DeepSeek::parse_http_error`]) into the #97
//!   [`ProviderErrorKind`] buckets (see `ProviderErrorKind`).
//!
//! It exposes a single construction seam ([`DeepSeek::new`]) and a lazy
//! `GET /models` cache ([`DeepSeek::list_models`] / [`DeepSeek::validate_model`]).
//! No app-level model name lives here: models are validated against the
//! provider's own `GET /models` list and travel on `ChatRequest.model` (filled
//! by the `Agent`'s `current_model`).

use std::collections::VecDeque;
use std::task::{Context, Poll};

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use tokio_util::sync::CancellationToken;

use crate::ai::protocol::{ChatRequest, ProviderError, ProviderErrorKind, StreamChunk};

use super::{Provider, ProviderStream};

/// A transport-level failure (no HTTP status): 'kind Upstream, code None'.
fn transport_error(e: impl std::fmt::Display) -> ProviderError {
    ProviderError {
        kind: ProviderErrorKind::Upstream,
        code: None,
        message: e.to_string(),
    }
}

/// A response body that carries a non-2xx status and may be either a JSON
/// error object (`{"error":{"message":...}}`, OpenAI-compatible) or a raw
/// string. Returns the extracted message, falling back to the raw text.
async fn error_message(response: reqwest::Response) -> String {
    let text = response.text().await.unwrap_or_default();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(message) = value.pointer("/error/message").and_then(|m| m.as_str()) {
            return message.to_string();
        }
    }
    text
}

/// Parses one `data:` SSE JSON payload into a [`StreamChunk`]. A delta holding
/// `content` is a `ContentDelta`; a delta holding `reasoning_content` is a
/// `ReasoningDelta`. An empty / unsupported delta yields `None`.
fn parse_delta(data: &str) -> Option<StreamChunk> {
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    let delta = value.pointer("/choices/0/delta")?;
    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            return Some(StreamChunk::ContentDelta(content.to_string()));
        }
    }
    if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
        if !reasoning.is_empty() {
            return Some(StreamChunk::ReasoningDelta(reasoning.to_string()));
        }
    }
    None
}

/// The stateful SSE adapter: buffers the upstream byte stream, splits it into
/// `data:` lines, yields a [`StreamChunk`] per delta, and emits `Done` only on
/// the `[DONE]` terminator. A stream that ends before `[DONE]` — or a cancelled
/// token — terminates the stream (a truncated `[DONE]` is a truncation error).
struct SseState {
    bytes: BoxStream<'static, Result<bytes::Bytes, reqwest::Error>>,
    buffer: Vec<u8>,
    pending: VecDeque<Result<StreamChunk, ProviderError>>,
    done: bool,
    cancel: CancellationToken,
}

impl SseState {
    fn poll_next(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<StreamChunk, ProviderError>>> {
        loop {
            if self.cancel.is_cancelled() {
                self.done = true;
                return Poll::Ready(None);
            }
            if let Some(item) = self.pending.pop_front() {
                return Poll::Ready(Some(item));
            }
            if self.done {
                return Poll::Ready(None);
            }
            match self.bytes.poll_next_unpin(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) => {
                    self.buffer.extend_from_slice(&chunk);
                    self.drain_lines();
                }
                Poll::Ready(Some(Err(e))) => {
                    self.pending.push_back(Err(ProviderError {
                        kind: ProviderErrorKind::Upstream,
                        code: None,
                        message: e.to_string(),
                    }));
                    self.done = true;
                }
                Poll::Ready(None) => {
                    // The upstream stream ended without `[DONE]`: truncation.
                    self.pending.push_back(Err(ProviderError {
                        kind: ProviderErrorKind::Upstream,
                        code: None,
                        message: "stream ended before [DONE]".to_string(),
                    }));
                    self.done = true;
                }
            }
        }
    }

    /// Extracts every complete `\n`-terminated `data:` line from the buffer,
    /// turning each into a pending chunk. A partial trailing line is left in
    /// the buffer until the next chunk arrives.
    fn drain_lines(&mut self) {
        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=pos).collect();
            line.pop(); // the `\n`
            while line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            let text = String::from_utf8(line).unwrap_or_default();
            let Some(data) = text.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                self.pending.push_back(Ok(StreamChunk::Done));
                self.done = true;
                // `[DONE]` is the terminator: ignore any trailing bytes in
                // this chunk rather than emitting deltas past `Done`.
                break;
            } else if let Some(chunk) = parse_delta(data) {
                self.pending.push_back(Ok(chunk));
            }
        }
    }
}

/// Wraps an upstream byte stream into a decoded [`ProviderStream`], honoring
/// `cancel` (termination) and the `[DONE]` terminator.
fn sse_stream(
    bytes: BoxStream<'static, Result<bytes::Bytes, reqwest::Error>>,
    cancel: CancellationToken,
) -> ProviderStream {
    let mut state = SseState {
        bytes,
        buffer: Vec::new(),
        pending: VecDeque::new(),
        done: false,
        cancel,
    };
    Box::pin(stream::poll_fn(move |cx| state.poll_next(cx)))
}

/// The DeepSeek connection config. `base_url` is the OpenAI-compatible root
/// (`https://api.deepseek.com`); the provider appends `/chat/completions` and
/// `/models`.
#[derive(Debug, Clone)]
pub struct DeepSeekConfig {
    pub api_key: String,
    pub base_url: String,
}

impl DeepSeekConfig {
    /// Reads `DEEPSEEK_API_KEY`, returning `None` when it is absent (AI
    /// disabled). `base_url` is fixed to the DeepSeek endpoint.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("DEEPSEEK_API_KEY").ok()?;
        Some(Self {
            api_key,
            base_url: "https://api.deepseek.com".to_string(),
        })
    }
}

/// The DeepSeek provider. Construction is pure — no network I/O; the lazy
/// [`OnceCell`] models cache is filled on the first [`DeepSeek::list_models`]
/// call and reused thereafter. The provider reads no model names of its own.
pub struct DeepSeek {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
    /// Lazy cache of `GET /models` result: fetched on first use, cached after,
    /// and evicted on failure so a later call retries.
    models: tokio::sync::OnceCell<Vec<String>>,
}

impl DeepSeek {
    /// Pure construction: stores the config and builds an HTTP client, but
    /// makes no network request.
    pub fn new(config: DeepSeekConfig) -> Self {
        Self {
            api_key: config.api_key,
            base_url: config.base_url,
            client: reqwest::Client::new(),
            models: tokio::sync::OnceCell::new(),
        }
    }

    /// The URL for a provider endpoint under `base_url`, tolerating a trailing
    /// slash.
    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }

    /// Queries `GET /models` for the provider's supported model list (the
    /// capability to validate a model name / fill a selector). Lazily fetched
    /// once and cached; a failed fetch is not cached, so a later call retries.
    pub async fn list_models(&self) -> Result<Vec<String>, ProviderError> {
        self.models
            .get_or_try_init(|| async {
                let url = self.endpoint("models");
                let response = self
                    .client
                    .get(&url)
                    .bearer_auth(&self.api_key)
                    .send()
                    .await
                    .map_err(transport_error)?;
                let status = response.status();
                if !status.is_success() {
                    return Err(Self::parse_http_error(
                        status.as_u16(),
                        error_message(response).await,
                    ));
                }
                let value: serde_json::Value = response.json().await.map_err(transport_error)?;
                let ids = value
                    .get("data")
                    .and_then(|d| d.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| item.get("id")?.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(ids)
            })
            .await
            .map(|models| models.clone())
    }

    /// Strictly validates that `model` exists in the provider's model list
    /// (fetching it lazily on first use). A model absent from the list is a
    /// [`ProviderErrorKind::Config`] failure.
    pub async fn validate_model(&self, model: &str) -> Result<(), ProviderError> {
        let models = self.list_models().await?;
        if models.iter().any(|m| m == model) {
            Ok(())
        } else {
            Err(ProviderError {
                kind: ProviderErrorKind::Config,
                code: None,
                message: format!("model '{model}' is not supported by this provider"),
            })
        }
    }

    /// Maps a DeepSeek HTTP error to a [`ProviderError`] (the #97 bucket):
    /// `400` (format) / `401` (auth) / `402` (balance) / `422` (params) are
    /// [`ProviderErrorKind::Config`]; `429` / `500` / `503` (and any unlisted
    /// status) are [`ProviderErrorKind::Upstream`]. The original status is
    /// kept as `code`.
    pub fn parse_http_error(code: u16, message: String) -> ProviderError {
        let kind = match code {
            400 | 401 | 402 | 422 => ProviderErrorKind::Config,
            _ => ProviderErrorKind::Upstream,
        };
        ProviderError {
            kind,
            code: Some(code),
            message,
        }
    }
}

#[async_trait]
impl Provider for DeepSeek {
    async fn stream_chat(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError> {
        // Gate on the model: an unknown model is a Config failure before any
        // chat cost. The list is fetched lazily on first use.
        self.validate_model(&req.model).await?;

        // `ChatRequest` already serializes to the OpenAI-compatible wire. We
        // only force `stream` on and drop an empty `tools` array.
        let mut body = serde_json::to_value(&req).map_err(|e| ProviderError {
            kind: ProviderErrorKind::Config,
            code: None,
            message: format!("failed to serialize request: {e}"),
        })?;
        if let Some(obj) = body.as_object_mut() {
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
            if obj
                .get("tools")
                .and_then(|t| t.as_array())
                .is_some_and(|a| a.is_empty())
            {
                obj.remove("tools");
            }
        }

        let url = self.endpoint("chat/completions");
        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(transport_error)?;

        let status = response.status();
        if !status.is_success() {
            return Err(Self::parse_http_error(
                status.as_u16(),
                error_message(response).await,
            ));
        }

        Ok(sse_stream(Box::pin(response.bytes_stream()), cancel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::protocol::{ContentBlock, Message};
    use futures::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn provider(server: &MockServer) -> DeepSeek {
        DeepSeek::new(DeepSeekConfig {
            api_key: "test-key".into(),
            base_url: server.uri(),
        })
    }

    fn chat_request(model: &str) -> ChatRequest {
        ChatRequest {
            messages: vec![
                Message::System {
                    content: "sys".into(),
                },
                Message::User {
                    content: vec![ContentBlock::Text("hi".into())],
                },
            ],
            model: model.into(),
            stream: true,
            tools: vec![],
        }
    }

    /// Awaits `stream_chat` and returns its `ProviderError`; a successful
    /// stream would be a test failure (the stream type isn't `Debug`).
    async fn stream_err(ds: &DeepSeek, req: ChatRequest) -> ProviderError {
        match ds.stream_chat(req, CancellationToken::new()).await {
            Err(e) => e,
            Ok(_) => panic!("expected stream_chat to fail"),
        }
    }

    /// A `GET /models` response template carrying the given model ids (the
    /// OpenAI-compatible `{object,data}` shape). Mounted inline per test.
    fn models_template(ids: &[&str]) -> ResponseTemplate {
        let data: Vec<serde_json::Value> = ids
            .iter()
            .map(|id| serde_json::json!({"id": id, "object": "model"}))
            .collect();
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"object": "list", "data": data}))
    }

    // --- parse_http_error (pure) ---

    #[test]
    fn parse_http_error_maps_config_codes() {
        for code in [400, 401, 402, 422] {
            let e = DeepSeek::parse_http_error(code, "msg".into());
            assert_eq!(e.kind, ProviderErrorKind::Config);
            assert_eq!(e.code, Some(code));
            assert_eq!(e.message, "msg");
        }
    }

    #[test]
    fn parse_http_error_maps_upstream_codes() {
        for code in [429, 500, 503] {
            let e = DeepSeek::parse_http_error(code, "msg".into());
            assert_eq!(e.kind, ProviderErrorKind::Upstream);
            assert_eq!(e.code, Some(code));
        }
    }

    #[test]
    fn parse_http_error_defaults_unknown_to_upstream() {
        let e = DeepSeek::parse_http_error(502, "msg".into());
        assert_eq!(e.kind, ProviderErrorKind::Upstream);
        assert_eq!(e.code, Some(502));
    }

    // --- list_models ---

    #[tokio::test]
    async fn list_models_returns_model_ids() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(models_template(&["deepseek-chat", "deepseek-reasoner"]))
            .mount(&server)
            .await;
        let models = provider(&server).list_models().await.unwrap();
        assert_eq!(
            models,
            vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()]
        );
    }

    #[tokio::test]
    async fn list_models_retries_after_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(models_template(&["m1"]))
            .mount(&server)
            .await;
        let ds = provider(&server);
        // First call fails and must not be cached.
        let first = ds.list_models().await.unwrap_err();
        assert_eq!(first.kind, ProviderErrorKind::Upstream);
        assert_eq!(first.code, Some(500));
        // Second call retries and succeeds.
        assert_eq!(ds.list_models().await.unwrap(), vec!["m1".to_string()]);
    }

    // --- validate_model ---

    #[tokio::test]
    async fn validate_model_accepts_known_model() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(models_template(&["deepseek-chat"]))
            .mount(&server)
            .await;
        assert!(
            provider(&server)
                .validate_model("deepseek-chat")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn validate_model_rejects_unknown_model() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(models_template(&["deepseek-chat"]))
            .mount(&server)
            .await;
        let err = provider(&server).validate_model("nope").await.unwrap_err();
        assert_eq!(err.kind, ProviderErrorKind::Config);
        assert_eq!(err.code, None);
    }

    // --- stream_chat ---

    #[tokio::test]
    async fn stream_chat_yields_reasoning_then_content_then_done() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(models_template(&["deepseek-chat"]))
            .mount(&server)
            .await;
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&server)
            .await;
        let mut stream = provider(&server)
            .stream_chat(chat_request("deepseek-chat"), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            stream.next().await,
            Some(Ok(StreamChunk::ReasoningDelta("think ".into())))
        );
        assert_eq!(
            stream.next().await,
            Some(Ok(StreamChunk::ContentDelta("hello".into())))
        );
        assert_eq!(stream.next().await, Some(Ok(StreamChunk::Done)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn stream_chat_maps_401_to_config() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(models_template(&["deepseek-chat"]))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(serde_json::json!({"error": {"message": "Invalid API key"}})),
            )
            .mount(&server)
            .await;
        let err = stream_err(&provider(&server), chat_request("deepseek-chat")).await;
        assert_eq!(err.kind, ProviderErrorKind::Config);
        assert_eq!(err.code, Some(401));
        assert_eq!(err.message, "Invalid API key");
    }

    #[tokio::test]
    async fn stream_chat_maps_500_to_upstream() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(models_template(&["deepseek-chat"]))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;
        let err = stream_err(&provider(&server), chat_request("deepseek-chat")).await;
        assert_eq!(err.kind, ProviderErrorKind::Upstream);
        assert_eq!(err.code, Some(500));
    }

    #[tokio::test]
    async fn stream_chat_rejects_unknown_model_before_chat() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(models_template(&["deepseek-chat"]))
            .mount(&server)
            .await;
        let err = stream_err(&provider(&server), chat_request("nope")).await;
        assert_eq!(err.kind, ProviderErrorKind::Config);
        assert_eq!(err.code, None);
    }

    #[tokio::test]
    async fn stream_chat_wire_drops_before_done_is_upstream_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(models_template(&["deepseek-chat"]))
            .mount(&server)
            .await;
        // The body is incomplete: no `[DONE]` terminator.
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&server)
            .await;
        let mut stream = provider(&server)
            .stream_chat(chat_request("deepseek-chat"), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            stream.next().await,
            Some(Ok(StreamChunk::ContentDelta("partial".into())))
        );
        match stream.next().await {
            Some(Err(e)) => {
                assert_eq!(e.kind, ProviderErrorKind::Upstream);
                assert_eq!(e.code, None);
            }
            other => panic!("expected an upstream truncation error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn connection_failure_is_upstream_with_no_code() {
        // Port 1 is a guaranteed-refused connection on localhost, so the
        // request fails before any HTTP response.
        let ds = DeepSeek::new(DeepSeekConfig {
            api_key: "k".into(),
            base_url: "http://127.0.0.1:1".into(),
        });
        let err = stream_err(&ds, chat_request("any")).await;
        assert_eq!(err.kind, ProviderErrorKind::Upstream);
        assert_eq!(err.code, None);
    }
}
