# 01: 后端 AI 运行时内核（protocol / provider / agent）

**What to build:** 建立通用 AI 运行时（`ai` 模块，与 `core` 解耦）：与供应商解耦的共享值类型、可插拔的 `Provider` seam、`Agent`/`Session` 骨架。`--test-ai-chat` CLI 自检入口先用 mock Provider 验证「发一条 User 消息 → 回一条 Assistant 回复」的单轮 `complete_once` 路径。本 ticket **不接扫雷、不接真实 HTTP**；真实 DeepSeek 实现不在本 ticket。


**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 用 mock `Provider` 构造 `Agent`，`complete_once` 发一条 User 消息并返回一条 Assistant 回复（含 `content`，可有 `reasoning_content`），报文吻合 `ChatRequest` 契约。
- [ ] `--test-ai-chat <str>` CLI 入口：仅能单独指定、与其它参数互斥（conflicts_with_all）；命中即早退、复用 `complete_once`；无 AI 配置（API key）时明确报错，不进正常产品流程。
- [ ] 共享值类型（`Message`/`ChatRequest`/`StreamChunk`/`ProviderError`/`ContentBlock`/`ToolCall`/`ToolDecl`）存在；`Message` 按 `role` 序列化成 wire 形状，`ProviderError` 序列化为 `{kind,code,message}`。
- [ ] `ProviderStream`（`Pin<Box<dyn Stream<Item=Result<StreamChunk,ProviderError>> + Send>>`）存在；`Provider::stream_chat(req, cancel) -> Result<ProviderStream, ProviderError>` 签名就绪，支持 `cancel`（`CancellationToken`）取消上游。
- [ ] `Agent` 提供 `stream`（流式）与 `complete_once`（聚合，内部复用 stream）；`set_model`/`current_model`/`current_provider` 可用，`ChatRequest.model` 由 `Agent` 填 `current_model`。

### 接口契约

```rust
// src/ai/protocol/mod.rs —— ai::protocol
// 本 ticket 引入的依赖（Cargo.toml）：futures（Stream/ProviderStream）、tokio-util（CancellationToken）
enum Message {
  System { content: String },
  User { content: Vec<ContentBlock> },
  Assistant { content: String, reasoning_content: Option<String>, tool_calls: Option<Vec<ToolCall>> },
  Tool { tool_call_id: String, content: String },
}
// serde(tag="role", rename_all="lowercase")
enum ContentBlock { Text(String), ImageUrl(String) }   // ImageUrl: data URL/base64 PNG; 仅 vision-exp, ≤384 token
//   wire 多模态形状与内部表示不同（{type,text} / {type,image_url,image_url:{url}}），实现期用 serde 标注/转换
struct ToolCall { id: String, name: String, arguments: serde_json::Value }   // model 请求调用（与 ToolDecl 相反方向）
struct ToolDecl { name: String, description: String, parameters: serde_json::Value }  // 喂给 model 的 tools 声明
struct ChatRequest { messages: Vec<Message>, model: String, stream: bool, tools: Vec<ToolDecl> }  // model 必填，由 Agent 填 current_model
enum StreamChunk { ReasoningDelta(String), ContentDelta(String), Done }   // Done=正常收尾；wire 上不出现（前端 [DONE] 收尾）
enum ProviderErrorKind { Config, Upstream }   // 序列化 "config"/"upstream"
struct ProviderError { kind: ProviderErrorKind, code: Option<u16>, message: String }   // wire 错误体 {kind,code,message}

// src/ai/provider/mod.rs —— ai::provider
type ProviderStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, ProviderError>> + Send>>;
trait Provider: Send + Sync {
  async fn stream_chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<ProviderStream, ProviderError>;
}

// src/ai/agent/mod.rs —— ai::agent
trait Tool: Send + Sync { fn decl(&self) -> ToolDecl; async fn call(&self, args: serde_json::Value) -> Result<String, String>; }
struct Session;  // impl { new(system: Message), push(&mut, Message), messages(&self) -> &[Message] }
enum AgentError { Provider(ProviderError), NoProvider, Cancelled }
struct ProviderSet;  // impl { new(), insert(name, Box<dyn Provider>), get(name), names() }
struct Agent;  // impl {
  //   new(providers: ProviderSet), set_model(&mut, model: String, provider: Option<&str>),
  //   current_provider(), current_model(), add_tool(Arc<dyn Tool>),
  //   stream(&self, session, cancel) -> Result<impl Stream<Item = Result<StreamChunk, AgentError>>, AgentError>,
  //   complete_once(&self, session, cancel) -> Result<Message, AgentError>,
  //   run_loop(&self, session, cancel) -> Result<Message, AgentError> }
```
