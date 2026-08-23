//! The agent session: a small loop that lets the model call tools (currently
//! just `read_board`), feeds each result back, and keeps going until it stops
//! calling tools or a turn cap is hit. Produces the transcript the /ai/analyze
//! endpoint returns.

use serde::Serialize;

use crate::ai::client::{ChatMessage, ChatRequest, LlmClient, LlmError};
use crate::ai::tools::read_board_tool;

const SYSTEM_PROMPT: &str = "You are an AI analyzing a Minesweeper board. \
You have a `read_board` tool that returns the current board as text. \
Call `read_board` to inspect the board, then analyze the position. Respond in Chinese.";

const INITIAL_USER: &str = "请查看当前扫雷棋盘并进行分析。";

/// The default turn cap for a session.
pub const DEFAULT_MAX_TURNS: usize = 5;

/// A tool call as shown in the transcript — the model's decision to read the
/// Board.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallView {
    pub name: String,
    pub arguments: String,
}

/// One model turn: its reasoning, its answer content, and any tool calls it
/// made.
#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub reasoning_content: Option<String>,
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCallView>,
}

/// The full transcript returned to the /ai/analyze caller.
#[derive(Debug, Clone, Serialize)]
pub struct SessionResult {
    pub steps: Vec<Turn>,
}

#[derive(Debug)]
pub enum SessionError {
    Llm(LlmError),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Llm(e) => write!(f, "{e}"),
        }
    }
}

impl From<LlmError> for SessionError {
    fn from(e: LlmError) -> Self {
        SessionError::Llm(e)
    }
}

/// Drives a single analysis session. `board_text` reads the current Board on
/// demand so the tool always sees the latest position; it is called only when
/// the model issues a `read_board` call.
///
/// The loop caps at `max_turns`, stops when the model no longer calls a tool,
/// and returns every turn so the caller can observe each decision.
pub async fn run_session<C: LlmClient>(
    client: &C,
    model: &str,
    reasoning_effort: &str,
    board_text: impl Fn() -> String + Send + Sync,
    max_turns: usize,
) -> Result<SessionResult, SessionError> {
    let mut messages = vec![
        ChatMessage {
            role: "system".into(),
            content: Some(SYSTEM_PROMPT.into()),
            ..Default::default()
        },
        ChatMessage {
            role: "user".into(),
            content: Some(INITIAL_USER.into()),
            ..Default::default()
        },
    ];
    let tools = vec![read_board_tool()];
    let mut steps = Vec::new();

    for _ in 0..max_turns {
        let request = ChatRequest {
            model,
            messages: &messages,
            tools: Some(&tools),
            thinking: Some(serde_json::json!({ "type": "enabled" })),
            reasoning_effort: Some(reasoning_effort),
            stream: false,
        };
        let reply = client.complete(&request).await?;

        steps.push(Turn {
            reasoning_content: reply.reasoning_content.clone(),
            content: reply.content.clone(),
            tool_calls: reply
                .tool_calls
                .as_ref()
                .map(|calls| {
                    calls
                        .iter()
                        .map(|c| ToolCallView {
                            name: c.function.name.clone(),
                            arguments: c.function.arguments.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        });

        match &reply.tool_calls {
            Some(calls) if !calls.is_empty() => {
                // Re-send the assistant turn WITHOUT reasoning_content (the
                // API ignores it; it only costs tokens) but WITH its
                // tool_calls, then append a tool result per call.
                messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: reply.content.clone(),
                    tool_calls: Some(calls.clone()),
                    ..Default::default()
                });
                for call in calls {
                    let result = match call.function.name.as_str() {
                        "read_board" => board_text(),
                        other => format!("unknown tool: {other}"),
                    };
                    messages.push(ChatMessage {
                        role: "tool".into(),
                        content: Some(result),
                        tool_call_id: Some(call.id.clone()),
                        ..Default::default()
                    });
                }
            }
            _ => break,
        }
    }

    Ok(SessionResult { steps })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use crate::ai::client::{
        ChatMessage, ChatRequest, LlmClient, LlmError, ToolCall, ToolFunction,
    };

    use super::run_session;

    struct FakeClient {
        responses: Mutex<VecDeque<ChatMessage>>,
    }

    impl FakeClient {
        fn new(responses: Vec<ChatMessage>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
            }
        }
    }

    #[async_trait]
    impl LlmClient for FakeClient {
        async fn complete(&self, _request: &ChatRequest<'_>) -> Result<ChatMessage, LlmError> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(LlmError::Empty)
        }
    }

    fn assistant(content: Option<&str>, calls: Option<Vec<ToolCall>>) -> ChatMessage {
        ChatMessage {
            role: "assistant".into(),
            content: content.map(str::to_owned),
            tool_calls: calls,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn session_reads_the_board_then_answers() {
        let client = FakeClient::new(vec![
            assistant(
                None,
                Some(vec![ToolCall {
                    id: "call_1".into(),
                    kind: "function".into(),
                    function: ToolFunction {
                        name: "read_board".into(),
                        arguments: "{}".into(),
                    },
                }]),
            ),
            assistant(Some("右侧区域较安全。"), None),
        ]);

        let reads = Arc::new(AtomicUsize::new(0));
        let reads_cb = reads.clone();
        let board_text = move || {
            reads_cb.fetch_add(1, Ordering::SeqCst);
            "board 9x9  game_state=playing".to_string()
        };

        let result = run_session(&client, "deepseek-v4-flash", "low", board_text, 5)
            .await
            .unwrap();

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.steps[0].tool_calls[0].name, "read_board");
        assert!(result.steps[0].content.is_none());
        assert_eq!(result.steps[1].content.as_deref(), Some("右侧区域较安全。"));
        // The Board was read exactly once, on the read_board call.
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn session_stops_when_the_model_stops_calling_tools() {
        let client = FakeClient::new(vec![assistant(Some("直接回答。"), None)]);
        let reads = Arc::new(AtomicUsize::new(0));
        let reads_cb = reads.clone();
        let board_text = move || {
            reads_cb.fetch_add(1, Ordering::SeqCst);
            String::new()
        };

        let result = run_session(&client, "deepseek-v4-flash", "low", board_text, 5)
            .await
            .unwrap();

        assert_eq!(result.steps.len(), 1);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
}
