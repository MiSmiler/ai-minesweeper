# Seams & 各 seam 的 pub 接口（草稿）

> 本文件被 `docs/spec-ai-guide-mode.md` 索引（见其 Testing Decisions「拟用 seams」一节）。
> 目标：给每个 seam 确定**基本形状**（pub 接口的类型/签名），供人工审阅、定稿。
> **形状层**——只列接口骨架与关键类型，不含实现；每个 seam 下用 `待确认` 标出需要你拍板的分叉点。
> 语言约定：**代码/类型一律 English**，说明性散文用中文；领域术语按 `CONTEXT.md` 词汇表。

相关 MDN 约定回顾：
- `core`（纯逻辑，无 serde/server 依赖）；`server`（th 传输层）；`ai`（通用 runtime，与 core 解耦）；
  `ai_adapter`（扫雷绑定，依赖 core+ai，**不依赖 server**）—— 见 **ADR-0013**。
- 前端 `app/`（组装）+ `game/` slice + `ai/` slice —— 见 **ADR-0011**（slice 内再分层）。
- **隐私硬约束**：发给 AI 的 payload 只含玩家可见状态，**绝不泄露 Mine 布局**。

### 目标目录树（后端 `src/` 与前端 `frontend/src/`；项内注明「已有/新增」）

```
后端 src/（目录 mod，不拆 crate）：
- core/        已有（Game 等，纯逻辑，无 serde/server 依赖）
- server/      已有（路由/wire）；ai_routes 为新增（/ai/... 薄传输）
- ai/          新增（与 core 解耦的通用 runtime）
  - protocol/  新增（共享协议/值类型：Message/ContentBlock/ToolCall/ToolDecl/ChatRequest/StreamChunk/ProviderError）
  - provider/  新增（访问机制：Provider trait + deepseek.rs）
  - agent/     新增（Agent/Session/Tool：run_loop）
- ai_adapter/  新增（扫雷绑定：BoardFormat/BoardView/system_prompt/Guide）
- main.rs      组合根（读 DEEPSEEK_API_KEY、构造 ProviderSet/Agent、挂 /ai/...）；另有一条 `--test-ai-chat` CLI 自检入口（命中即早退、复用 Agent::complete_once，不进正常流程）

前端 frontend/src/：
- app/    组合层（ADR-0011/0012）：
          · main.ts      —— 入口：读初始 mode → mountMode + renderModeSwitcher + onSwitch（改造自现 main.ts）
          · mode.ts      —— PlayModeName / AppDeps / mountMode（分发）+ renderModeSwitcher（UI 控件）
          · singleMode.ts—— composeSingleMode（从现 main.ts 抽出 single 组合；供 mountMode("single")）
          · guideMode.ts —— composeGuideMode（新增，拼 ai/ slice）
- game/   已有（api/client/render/interaction）
- ai/     新增（api.ts/stateMachine.ts/conversation.ts/axis.ts/screenshot.ts）
- infra/  已有（log/testUtils）
```

---

## 后端 seams

### S1 `core::Game`（复用，**无新增接口**）

参考 seam。顾问只读 `core::Game` 的**可见 API**，用于构建玩家可见视图：

```rust
// 已有，只列被顾问用到的可见方法（不是本 map 要新增的）
impl Game {
    pub fn game_state(&self) -> GameState;      // ready/playing/won/lost
    pub fn difficulty(&self) -> Difficulty;     // size() / mine_count()
    pub fn size(&self) -> BoardSize;
    pub fn flags_remaining(&self) -> i32;       // 可为负（over-flag）
    pub fn cell_view(&self, pos: Position) -> CellView;  // state + revealed content
}
```

- `GameSnapshot::from_game`（`server::wire`）是「玩家可见投影」的既有先例；`ai_adapter` 是**另一个**读
  `core` 可见 API 的平行 reader，**不会**依赖 `server::wire`。
- **已确认**：不给 `core::Game` 加新的 `snapshot()` / 其它可见方法——`cell_view` + `flags_remaining`
  + `difficulty().mine_count()` 已够拼出 #94 头部与棋盘。

### S2 `ai::protocol` —— 共享协议/值类型

与供应商**解耦**的共享契约/词表，**无 IO、无 HTTP**；`provider`/`agent`/`ai_adapter`/`server` 都引用。
`ai_adapter::Guide::suggest` 与未来 `ai_play` 都经它——它只承载值类型，作为 AI 层**共用契约**独立列出。

文件：`src/ai/protocol/`（目录 mod，`mod.rs`）。

**实现注记**：本 spec 阶段 `main` 先 **hardcode `DeepSeek`**（`DeepSeek::new(config, default_model)`），**不**实现
`--provider`/`--model` CLI 选择——那只是为将来留口。但构造点收在 `main`、上层只见 `Box<dyn Provider>`，将来
要加 CLI 时只需在 `main` 的 provider 选择处扩展（用 `--model` 值构造/选择 `Agent` 的初始 model），`ai_adapter`/`Guide`/`ai::protocol` 零改动。
`main` 启动时可**可选预热**模型列表（`let _ = deepseek.list_models().await;`，失败忽略、不阻断启动）。

```rust
// ai::protocol/mod.rs —— 共享协议/值类型（与供应商解耦；provider/agent/ai_adapter/server 共用）
use serde::Serialize;

/// 模型名就是 provider 特有的「名字」字符串——不写死 enum。是否属于该 provider 由 provider 端校验；
/// 支持列表经 `GET /models`。model 是 `ChatRequest` 的**必填字段**，由 `Agent` 用自记录的 `current_model` 填入；
/// `DeepSeek` 本身只读、不自持 model。

/// 对话消息（按 role 区分字段集合）。`serde(tag="role")` 直接序列化成 wire 形状。
/// 4 个变体覆盖 system / user / assistant / tool。
#[derive(serde::Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System { content: String },
    /// 可多模态（文本/图）。
    User { content: Vec<ContentBlock> },
    Assistant {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")] reasoning_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")] tool_calls: Option<Vec<ToolCall>>,
    },
    Tool { tool_call_id: String, content: String },
}
#[derive(serde::Serialize)]
pub enum ContentBlock {
    Text(String),
    /// data URL / base64 的 PNG；仅 vision-exp，单图 ≤384 token（#92）。wire 多模态形状（`{type,text}` /
    /// `{type,image_url,image_url:{url}}`）与内部表示不同，实现期用 serde 标注/转换。
    ImageUrl(String),
}
/// model 请求调用某工具（function calling）。与 `ToolDecl` 是**相反方向**：`ToolDecl` 是「声明」
/// （喂给 model 的 tools 数组），`ToolCall` 是「model 请求调用」（assistant.tool_calls 的一项），
/// 按 `name` 衔接 `Tool`。
pub struct ToolCall { pub id: String, pub name: String, pub arguments: serde_json::Value }

/// 工具「声明」（未来 AiPlay 用；顾问模式传空集）—— 与 `ToolCall`（model 请求调用）相对。
pub struct ToolDecl {
    pub name: String, pub description: String,
    pub parameters: serde_json::Value,   // JSON Schema
}

pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: String,          // 必填：当前 model；由 Agent 填 current_model（provider 端直接用它）
    pub stream: bool,           // 顾问恒 true（SSE）
    pub tools: Vec<ToolDecl>,   // 顾问空集
}

/// 共享「流内容」块（各层通用；reasoning / content 分开 —— #95 双流；`Done` 为正常收尾）。
/// 中断/错误不在此，走各层 `Result<StreamChunk, E>` 的 `Err`。
pub enum StreamChunk {
    ReasoningDelta(String),
    ContentDelta(String),
    Done,                       // 正常收尾；wire 上不出现（前端 [DONE] 收尾）
}

/// 前置失败（未流任何内容之前）—— 映射到 #97 的 bucket。
/// 通用错误载体，**不写死错误类型枚举**（同 `Model` 是字符串的教训）：具体错误码及含义由 provider
/// （deepseek）给出，抽象层只关心跨 provider 的 `kind` + 原始码/描述。`code` 开集，新增错误码不改此类型。
/// 它本身也是发送给前端的 wire 错误体（序列化 { kind, code, message }）。
#[derive(serde::Serialize)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,   // Config | Upstream（序列化为 "config"/"upstream"）
    pub code: Option<u16>,         // provider 原生的 HTTP 错误码；无则 None（序列化为 null）
    pub message: String,           // 可读描述（DeepSeek 错误 body）
}
/// 「前置失败」的两个 bucket（#97 ①）。`ProviderError`（wire 错误体）序列化时用它（"config"/"upstream"）。
#[derive(serde::Serialize)]
pub enum ProviderErrorKind { Config, Upstream }
```

### S3 `ai::provider` —— `Provider` trait + `deepseek`

访问供应商的机制（依赖 `ai::protocol` 的 `ChatRequest`/`StreamChunk`/`ProviderError`）。`agent` 是唯一内部消费者，
测它时注入 mock `Provider`；`main` 作为组合根读 `DEEPSEEK_API_KEY`、构造 `DeepSeek`。`Provider`/`deepseek` 均对外
`pub`（`pub mod deepseek`）；抽象 seam 不反向依赖 `deepseek`。

```rust
// ai::provider/mod.rs —— 访问供应商的机制：Provider trait + ProviderStream
use crate::protocol::{ChatRequest, StreamChunk, ProviderError};
use futures::Stream;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

/// 上游生成是否已被取消（用户中断 / 超时 / 上游错误），由调用侧 `select!` 驱动。
/// 正常项 = `StreamChunk`（delta / Done）；流中错误/中断 = `Err(ProviderError)`。
pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, ProviderError>> + Send>>;

pub trait Provider: Send + Sync {
    /// 前置失败返回 `Err`（映射 #97 ①）：provider 端先 `validate_model(req.model)`（model 不属于它 → `Config`）。
    /// 通过后返回流（`Item = Result<StreamChunk, ProviderError>`）：正常项 = reasoning/content delta + `Done`。
    /// 生成期间可通过 `cancel` 取消（用户中断 #97）——取消表现为流提前 error/结束；
    /// 流中上游错误也走 `Err(ProviderError)`（供 guide 映射 `Interrupt(reason)`）。
    async fn stream_chat(
        &self, req: ChatRequest, cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError>;
}
```

```rust
// ai/provider/deepseek.rs —— 具体实现（pub，供 main 构造/选择；抽象 seam 不引用它）
pub struct DeepSeek {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
    /// lazy 缓存 `GET /models` 结果：首次用到才拉，失败可重试，一次填充、多次复用。
    models: tokio::sync::OnceCell<Vec<String>>,
}
pub struct DeepSeekConfig { pub api_key: String, pub base_url: String }
impl DeepSeek {
    /// 读 env `DEEPSEEK_API_KEY` 构造；无 key 时说明 AI 不启用。纯构造，不发网络请求。
    /// provider 只读、不自持 model；model 随 `ChatRequest.model`（Agent 填）。
    pub fn new(config: DeepSeekConfig) -> Self;
    /// `GET /models` 查询本 provider 支持的模型（可选 capability：校验模型名 / 填充选择器）。
    /// lazy：首次调用经 `OnceCell::get_or_try_init` 拉取并缓存；失败不缓存、下次重试。
    pub async fn list_models(&self) -> Result<Vec<String>, ProviderError>;
    /// 严格校验：模型必须存在于本 provider 的模型列表（首次经 `list_models` 拉取/缓存）。
    pub async fn validate_model(&self, model: &str) -> Result<(), ProviderError>;
    /// 把 DeepSeek 的 HTTP 错误响应解析成 `ProviderError`——错误码及含义归本 mod：
    /// 400 格式 / 401 认证失败 / 402 余额不足 / 422 参数错误 → `Config`；429 / 500 / 503 → `Upstream`。
    pub fn parse_http_error(code: u16, message: String) -> ProviderError;
}
impl Provider for DeepSeek {
    // 入口先 `validate_model(&req.model)`：model 不在本 provider 列表 → `Err(Config)`（首次经 list_models 拉取/缓存）。
    // 校验通过后发请求，用 `req.model`（Agent 填入的 current_model）；无 enum、无我们维护的映射表。
    // 连接失败/超时（无 HTTP 码）→ 直接构造 ProviderError{ kind: Upstream, code: None, message }。
}
```

- 依赖新增：`futures`（Stream）、`tokio-util`（CancellationToken）、`reqwest`（deepseek 实现）。
- **已确认**：`ProviderError` 是通用载体（`kind` 字段，非 `kind()` 方法）；`ProviderErrorKind::{Config,Upstream}`
  由 `deepseek::parse_http_error` 判定，`server` 直接序列化 `ProviderError`，不再靠 code 分支。

### S4 `ai::agent` —— `Tool` / `Session` / `Agent`（run_loop）

ADR-0013 的 `agent → provider`。顾问用 `complete_once`（单轮、无工具）；`run_loop`/`Tool` 为未来 `AiPlay` 预留，但**接口一并定义**（按你的决定）。

```rust
// ai::agent
use crate::protocol::{Message, ToolCall, ToolDecl};
use crate::provider::Provider;
use futures::Stream;

/// 一个工具（未来 AiPlay；顾问空集）。
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn decl(&self) -> ToolDecl;
    async fn call(&self, args: serde_json::Value) -> Result<String, String>;  // 结果即 `Message::Tool.content` 文本
}

/// 一轮对话的增量历史（只追加）。
pub struct Session { /* Vec<Message> */ }
impl Session {
    pub fn new(system: Message) -> Self;          // Message::System（system prompt）
    pub fn push(&mut self, msg: Message);
    pub fn messages(&self) -> &[Message];
}

// ai::agent 不定义独立事件类型：`Agent::stream` 的项 = `Result<StreamChunk, AgentError>`。
// 正常项（`StreamChunk`）与 guide / ai_play 共享；中断/错误走 `Err(AgentError)`。

pub enum AgentError {
    Provider(ProviderError),
    NoProvider,     // stream 时 current_provider 为空 / 容器无此 provider（未 set_model 或切错名）
    Cancelled,    // 流被打断（#97 ②）。目前仅由用户手动 cancel（server::handle_user_interrupt）触发 → guide 映射 `Interrupt(UserInterrupt)`；
                  // 将来可能出现非用户也 cancel 当前流（如切 mode/新局/超时掐断）——届时需区分来源。上游流中错误走 `Provider(ProviderError)`。
}

/// 按名字注册多个 provider 的容器（保序）。provider 只读；多 provider 为将来留口，本 map 只用 DeepSeek。
pub struct ProviderSet { providers: Vec<(String, Box<dyn Provider>)> }
impl ProviderSet {
    pub fn new() -> Self;
    pub fn insert(&mut self, name: impl Into<String>, provider: Box<dyn Provider>);
    pub fn get(&self, name: &str) -> Option<&dyn Provider>;
    pub fn names(&self) -> Vec<&str>;
}

pub struct Agent {
    providers: ProviderSet,
    current_provider: String,   // 当前用哪个 provider（名字串，"deepseek"）
    current_model: String,      // 当前用哪个 model（属 current_provider）
    tools: Arc<Vec<Tool>>,
}
impl Agent {
    /// 只收 provider 容器；current 初始为空，随后由调用者 `set_model` 定（V2）。
    pub fn new(providers: ProviderSet) -> Self;
    /// 设当前选择：Some(p) = 切到 provider p 并设 current_model；None = 只设 current_model（沿用当前 provider）。
    pub fn set_model(&mut self, model: String, provider: Option<&str>);
    pub fn current_provider(&self) -> &str;
    pub fn current_model(&self) -> &str;
    pub fn add_tool(&mut self, tool: Arc<dyn Tool>);          // 预留给 AiPlay
    /// 流式：把 session 发出去（`ChatRequest.model` 填 `current_model`），边收流边转发 `StreamChunk`。
    /// 外层 `Err` = 前置失败：`NoProvider`（查不到 current_provider）/ `Provider(ProviderError)`。
    /// 内层 `Err(AgentError)` = 流中：`Cancelled`（用户 cancel）/ `Provider(ProviderError)`（上游错误）。
    /// 生成期间可由 `cancel` 中断（#97）。
    pub async fn stream(
        &self, session: &Session, cancel: CancellationToken,
    ) -> Result<impl Stream<Item = Result<StreamChunk, AgentError>>, AgentError>;
    /// 单轮完成（无工具循环）—— 顾问路径。
    /// 也是 dev 手工测试 `--test-ai-chat` 的入口（聚合、非流式；无需 `Provider` 新增接口）。
    /// 返回 `Message::Assistant`（`tool_calls: None` = 最终完整答复）。
    pub async fn complete_once(
        &self, session: &Session, cancel: CancellationToken,
    ) -> Result<Message, AgentError>;
    /// 持久工具循环 —— 未来 AiPlay；本 map 不用。
    /// 循环内先后 push `Message::Assistant`（带 `tool_calls`）、`Message::Tool`（结果回填），
    /// 最终同样返回 `Message::Assistant`（`tool_calls: None`）。
    pub async fn run_loop(
        &self, session: &Session, cancel: CancellationToken,
    ) -> Result<Message, AgentError>;
}
```

- **已确认**：`Agent` 同时提供 `stream`（流式）与 `complete_once`（聚合，复用 `stream` 内部）。`suggest` 走 `stream`（流式给前端），`--test-ai-chat` 走 `complete_once`（聚合）。两条路并存、职责清晰。

### S5 `ai_adapter` —— `BoardFormat` / `BoardView` / `system_prompt` / `build_*` / `Guide::suggest`

扫雷绑定：把 `core::Game` 的**可见**侧渲染成 #94 4 形式 + 拼 system prompt；`Guide::suggest` 是用户「点问 AI」的入口。

```rust
// ai_adapter（依赖 core + ai；不依赖 server）
use crate::core::{Game, CellView, Difficulty, GameState, Position};
use crate::ai::protocol::{ContentBlock, Message};
use crate::ai::agent::{Agent, AgentError, Tool};

/// #94 4 种呈现形式。wire 序列化 = kebab-case（`#[serde(rename_all = "kebab-case")]`），
/// 与前端 `ai/api.ts` 的 kebab 字面量对齐：`SimpleText → "simple-text"`、`FullCoordinates → "full-coordinates"`。
/// 它是 POST /ai/guide/:id 请求体字段（前端传回），故需 `Deserialize`（与 `GuideRequest` 一起）。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoardFormat { SimpleText, Emoji, FullCoordinates, Image }

/// 玩家可见棋盘视图；**绝不携带 Mine 布局**（隐私硬约束）。
pub struct BoardView {
    pub difficulty: Difficulty,
    pub state: GameState,
    pub rows: usize, pub cols: usize,
    pub mine_count: usize,       // recipe 常量 = difficulty.mine_count()
    pub flags_remaining: i32,    // 可为负（over-flag）
    pub cells: Vec<CellView>,    // 仅可见
}
impl BoardView {
    /// 只读 cell_view / game_state / difficulty / size / flags_remaining —— 隐私 seam。
    pub fn from_game(game: &Game) -> Self;
}

/// #94/#95 共享 system prompt（坐标 0-based、只看可见、末尾 SUGGEST 契约）。纯。
pub fn system_prompt() -> String;

/// 文本形式（A/B/C）的 user 消息体。纯。
pub fn build_text_blocks(view: &BoardView, format: BoardFormat) -> Vec<ContentBlock>;
/// 图像形式（D）的 user 消息体 = 头部文本 + 截图 data URL。纯。
pub fn build_image_blocks(view: &BoardView, image_data_url: &str) -> Vec<ContentBlock>;

// ai_adapter 不定义独立事件类型：`Guide::suggest` 的项 = `Result<StreamChunk, InterruptReason>`。
// 正常项（`StreamChunk`）+ 流中中断 `Err(InterruptReason)`；前置失败走外层 `Err(SuggestPreFlightError)`。

/// 终止原因（#97）。wire / 前端镜像此值。
pub enum InterruptReason { UserInterrupt, RateLimit, Timeout, UpstreamError, Unknown }

/// 前置失败 → bucket（#97 ①），走 HTTP 状态码 + 结构化错误体。
pub struct SuggestPreFlightError(AgentError);

/// 请求：前端只发 format（+ 图像形式附带 `image_data_url`）。文本形式的棋盘由后端读自己的 Game；**不带 model**（后端 `DeepSeek` 默认）。
pub struct GuideRequest {
    pub format: BoardFormat,
    /// 仅 image 形式：前端 `html-to-image` 产出的 PNG data URL（含 `data:image/png;base64,` 前缀）。
    pub image_data_url: Option<String>,
}

/// 一次性顾问：注入棋盘、走一轮、流式返回（`Ok(StreamChunk)` 推进 / `Err(InterruptReason)` 中断）。
/// 前置失败 → `Err(SuggestPreFlightError)`；否则返回流（`Item = Result<StreamChunk, InterruptReason>`）。
/// 默认 model（非图像形式）；suggest 每次按 format 显式设，避免跨请求状态遗留。#92/#95
const DEFAULT_MODEL: &str = "deepseek-v4-flash";
/// 多模态（vision）model——format D（Image）时切换（`set_model(VISION_MODEL)`）。#92
const VISION_MODEL: &str = "deepseek-v4-flash-vision-exp";

/// 多线程共享：`Arc<Mutex<Agent>>`——`&self` 方法内 `lock()` 拿到 `&mut Agent` 就能 `set_model`（只改 model 记录）。
pub struct Guide { agent: Arc<Mutex<Agent>> }
impl Guide {
    /// 用共享 Agent（DeepSeek 或 mock 构造）构造 Guide。
    pub fn new(agent: Arc<Mutex<Agent>>) -> Self;
    /// 注入棋盘、走一轮、流式返回；`cancel` 可中断上游生成（#97）。
    pub async fn suggest(
        &self, game: &Game, req: GuideRequest, cancel: CancellationToken,
    ) -> Result<impl Stream<Item = Result<StreamChunk, InterruptReason>>, SuggestPreFlightError>;
    // 内部：let mut agent = self.agent.lock().unwrap();
    //       req.format == Image → agent.set_model(VISION_MODEL, None)
    //       否则               → agent.set_model(DEFAULT_MODEL, None)  // 每次按 format 显式设，避免跨请求遗留
    //       image 形式：先把 `req.image_data_url` 留底写盘（<exe_dir>/base64_img/YYYYMMDD_<seed>_<seq>.png，
    //         写失败不阻断发送），再构造 image blocks——留底是 suggest 的内部副作用，不加 pub 接口。
    //       build 棋盘消息 → Session(Message::System/User) → agent.stream()（agent 自动填 current_model）
    //       `Ok(StreamChunk)` 透传；`Err(AgentError::Cancelled)`（用户 cancel）→ `Err(Interrupt(UserInterrupt))`；
    //       `Err(AgentError::Provider(pe))`（上游流中错误）→ 按 `pe` 折射 `Err(Interrupt(rate_limit/timeout/upstream))`；
    //       外层前置失败 → `Err(SuggestPreFlightError)`。
    // 注：lock() 借 `&mut Agent` 调 set_model；stream 是 `&self`，同 guard 下可续用。
    // 无并发需求：同刻只运行一个 `running` 的 `suggest`（前端 `AnalysisPhase.running` 保证），故不做并发安全设计
    // （不引入 `&mut self`/额外锁）；Session 局部、model 按 format 显式设，每次调用状态自洽。
}

/// 访问一个游戏实例可见状态的句柄（ADR-0013：工具绑定是参数而非写死单一 Game，为未来 AiPlayWithMe 双 Game 留口）。
/// TODO: 现实现 = 封装 `Arc<Mutex<core::Game>>`（单 Game）；细节待 AiPlay 落地时补。本 map 工具集为空。
pub struct GameHandle { /* TODO */ }

/// 工具绑定（未来 AiPlay；顾问传空集）。绑到 GameHandle 以便未来双 Game 不换 adapter。
pub fn tools(handle: &GameHandle) -> Vec<Arc<dyn Tool>>;
```

- **隐私**：`BoardView::from_game` 是隐私边界——只调 `cell_view`/`game_state`/`difficulty`/`size`/
  `flags_remaining`，**不**读 `mines`。测它时断言 payload 不含任何 mine 位置。
- **定案**：
  - 文本形式**由后端读自己的 `Game` 渲染**、前端只发 `format`（+`image`），**不带 `model`**——后端 Game 权威、前端薄、
    隐私（snapshot）收紧在后端（`BoardView::from_game`）。spec §10/用户故事措辞已统一为「后端读自己的 `Game` 渲染」。
  - `SUGGEST {"row":N,"col":M}` **不做任何解析**（#95「永不解析、人读文本」）。事件流里**没有**坐标字段；
    `SUGGEST`/`SUGGEST null` 只是 content 末尾一段文本。**不需要**后端「报告」解析出的坐标。
  - `model` 暴露：**定案**——前端不带，后端 `DeepSeek` 默认（`GuideRequest.model` 删；`#96` 未提模型选择器）。
    `--model` CLI **不在本 map 实现**（留口：将来在 `main` 选择处扩展；`ai_adapter`/`Guide` 零改动）。

### S6 `server` —— `/ai/...` 传输 seam

薄传输层：把 `ai_adapter::Guide::suggest` 的事件流 SSH 转发给前端，并处理中断/终止。

```rust
// server
pub fn ai_routes(state: Arc<AppState>) -> Router;
//  POST /ai/guide/:id        → SSE 流（GuideEventDto…，收 [DONE] 结束）
//  POST /ai/guide/:id/interrupt → 取消上游，驱动同 SSE 的 {reason:"user_interrupt"}

/// SSE 流上的 wire 事件（消费 `Guide::suggest` 的 `Result<StreamChunk, InterruptReason>` 映射）：
/// `Ok(ReasoningDelta/ContentDelta)` → data；`Ok(Done)` → 发 `[DONE]`；`Err(reason)` → `Interrupt`。
/// 带 `kind` 序列化（`#[serde(tag="kind")]`），与前端 `GuideEvent`(TS) 同构：
/// `{kind:"reasoning",text}` / `{kind:"content",text}` / `{kind:"interrupt",reason}`。
/// 注：`Reasoning`/`Content` 的 payload 需映射为 `text` 字段（变体改带字段或自定义 `Serialize`）。
pub enum GuideEventDto {
    Reasoning(String),
    Content(String),
    Interrupt { reason: InterruptReason },   // 末端中断 event（#97）
}
// 注：[DONE] 是 SSE 流的收官标记（wire 无 Done 变体）；`Interrupt` 是「未收 [DONE]」时的显式中断。

/// 前置失败的错误体 = `ai::protocol::ProviderError`（带 `Serialize`，序列化为 { kind, code, message }）。
/// 不再单独定义 wire 错误体；`server` 直接把 `ProviderError` 序列化返回给前端。

/// 路由处理逻辑（与现在测 apply_action 同思路，可独立测）。
pub(crate) fn handle_guide(...) -> impl IntoResponse;   // SSE
pub(crate) fn handle_user_interrupt(...) -> impl IntoResponse;
```

- **定案**：分析会话 id（`sessionId`）由**前端**生成（如 `crypto.randomUUID()`）——中断由前端发起、前端持有该分析会话；
  `POST /ai/guide/:id` 带上它、`POST /ai/guide/:id/interrupt` 复用同 `sessionId`，`server` 用 `<id>` 关联该 SSE 的 `cancel`。（`:id` = `sessionId`）
- **定案**：`GuideEventDto` 带 `kind`（`#[serde(tag="kind")]`），与前端 `GuideEvent`(TS)（`{kind:"reasoning"/"content"/"interrupt"}`）同构。
  实现：`Reasoning`/`Content` 的 payload 映射为 `text` 字段（变体改带字段或自定义 `Serialize`），产出 `{kind,text}`。

---

## 前端 seams

### S7 `ai/api.ts` —— wire 契约（镜像后端 `/ai/...`）

```ts
// ai/api.ts
export type BoardFormat = "simple-text" | "emoji" | "full-coordinates" | "image";
// model 是 provider 特有的名字字符串（"deepseek-v4-flash" 等），不写死 union。
export type InterruptReason =
  | "user_interrupt" | "rate_limit" | "timeout" | "upstream_error" | "unknown";

// 前端消费的 wire 事件（对应后端 `GuideEventDto`）：`sse_done` 由前端读到 SSE `[DONE]` 时合成，
// wire 上不产 `sse_done`（后端 `GuideEventDto` 无 `Done` 变体）。后端 domain 事件是共享的 `StreamChunk`
// （`Ok(StreamChunk)`=delta/Done；流中中断走 `Err(InterruptReason)`），前端这里把它译成 wire 事件。
export type GuideEvent =
  | { kind: "reasoning"; text: string }
  | { kind: "content"; text: string }
  | { kind: "sse_done" }
  | { kind: "interrupt"; reason: InterruptReason };

export type ProviderError = { kind: "config" | "upstream"; code: number | null; message: string };   // = 后端 `ai::protocol::ProviderError`（wire 错误体）

export interface GuideRequest { format: BoardFormat; imageDataUrl?: string; }   // 不带 model（后端 DeepSeek 默认）

export interface AiApi {
  /** POST /ai/guide/:id —— 消费 SSE 流（`:id` = `sessionId`），逐 event 回调；前置失败走 onProviderError。 */
  startGuide(sessionId: string, req: GuideRequest, onEvent: (e: GuideEvent) => void, onProviderError: (e: ProviderError) => void): void;
  /** POST /ai/guide/:id/interrupt —— 用户主动取消。 */
  interrupt_by_user(sessionId: string): Promise<unknown>;
}
```

- 前端**不解析** `SUGGEST`（#95）；`GuideEvent` 只有 reasoning/content/sse_done/interrupt，**没有坐标字段**。
  image 形式的 base64 由 `ai/screenshot.ts` 收集后放进 `GuideRequest.imageDataUrl`。

### S8 `ai/stateMachine.ts` —— 分析状态机

持有一次分析的生命周期（#97 失败两型 + 用户中断）。

```ts
// ai/stateMachine.ts
export type AnalysisPhase =
  | "idle" | "running" | "done" | "interrupted" | "preflight-failed";

export interface AnalysisState {
  phase: AnalysisPhase;
  reasoning: string;      // 累积
  content: string;        // 累积（含末尾 SUGGEST 行）
  interruptReason?: InterruptReason;   // interrupted 时
  providerError?: ProviderError;     // preflight-failed 时
}

export interface AnalysisMachine {
  /** 发起一轮分析；每轮 = 最新棋盘（不缓存失败快照，#97）。 */
  start(req: GuideRequest): void;
  /** 用户主动取消：POST /ai/guide/:id/interrupt（前端保持 SSE 不 abort，#97）。 */
  interrupt_by_user(): Promise<void>;
  /** 输入格式变更 / 新局 / 切 mode 时清空（历史清除语义，见 `app/` 组装）。 */
  reset(): void;
  /** 订阅状态变化，返回退订。 */
  onState(cb: (s: AnalysisState) => void): () => void;
}

export function createAnalysisMachine(deps: { api: AiApi; newSessionId: () => string }): AnalysisMachine;
```

- 历史绑定一局、分析中不可点、输入格式变更 → 确认 + 清历史（#96）—— 这些判定放组装层（`app/`），状态机只负责
  `running/done/interrupted/preflight-failed` 及累积文本。

### S9 `ai/conversation.ts` —— 双流对话渲染（#95）

```ts
// ai/conversation.ts
export function createConversation(container: HTMLElement): {
  render(state: AnalysisState): void;
};
```
- `reasoning_content` → 浅色小字、整块可折叠；`content` → 正常字体、不折叠（仿 DeepSeek 网页版）。
- `SUGGEST`/`SUGGEST null` 只是 content 文本，**不解析、不高亮**。

### S10 `ai/axis.ts` —— 行列号辅助标记（#111）

```ts
// ai/axis.ts
export interface AxisOverlay {
  setRowsCols(rows: number, cols: number): void;
  setVisible(visible: boolean): void;
  /** 销毁 overlay（切 mode 重置）。 */
  destroy(): void;
}
export function createBoardAxis(boardEl: HTMLElement, opts?: { visible?: boolean }): AxisOverlay;
```
- `.board` 外**绝对定位 overlay**、`pointer-events:none`、**不进 `.board` 截图**、对 4 形式零影响。
- 默认关、guide 组件内状态（切走即丢、不持久化）。checkbox「行列号」落仪表盘（组装层）。

### S11 `ai/screenshot.ts` —— `html-to-image` 截图（#93）

```ts
// ai/screenshot.ts
export async function captureBoardImage(
  boardEl: HTMLElement, opts?: { pixelRatio?: number },
): Promise<string>;   // PNG data URL（默认 pixelRatio 不放大）
```
- 供 `ai/api.ts` `GuideRequest.imageDataUrl` 用；Playwright 只作开发/工具截图，不作 runtime capture。

### S12 `app/` 组装 seam（Mode switcher + `PlayMode` 组合）

`app/` 是 mode 组合处（ADR-0011/0012）。guide 组合拿 `game/` slice + `ai/` slice 拼。

```ts
// app/
export type PlayModeName = "single" | "ai-guide";

/// session 策略（#96「8 项」= 4 形式 × 本 2 策略）：per-analysis（一次分析一个新上下文，已实现）/
/// per-game（同 session 内多次分析拼接上下文，**未实现**，UI 置灰 + 标注「(未实现)」）。
/// 仅组装层 UI 概念，本期不传给 AiApi/AnalysisMachine/Guide。
export type SessionStrategy = "per-analysis" | "per-game";

/// `captureBoardImage`（S11 `screenshot.ts`）的函数类型：截图棋盘 PNG（data URL）。
export type CaptureBoardImage = (boardEl: HTMLElement, opts?: { pixelRatio?: number }) => Promise<string>;

export interface AppDeps {
  getPlayMode(): PlayModeName;
  // 复用 game slice 的既有入口（createGameClient 由各 mode 实例化）
  // ai/ slice 的入口（由 composeGuideMode 用）
  aiApi: AiApi;
  // 注入 captureBoardImage：它是浏览器真实截图（html-to-image），测试环境（jsdom）无法真正截图，
  // 必须依赖注入以便 stub。轴标（createBoardAxis）是纯 DOM、测试可跑，直接 import，不进 AppDeps。
  captureBoardImage: CaptureBoardImage;
}

/** 挂载某个 mode 的组合，返回卸载函数。切 mode 即调用当前卸载 + 挂载新的。 */
export function mountMode(mode: PlayModeName, root: HTMLElement, deps: AppDeps): () => void;

/** 顶栏 mode-switcher；切 mode 触发 onSwitch。 */
export function renderModeSwitcher(root: HTMLElement, current: PlayModeName, onSwitch: (m: PlayModeName) => void): void;

/** 组装 guide 模式（独立 DOM + 独立 game client + ai/ slice）。 */
export function composeGuideMode(root: HTMLElement, deps: AppDeps): { dispose(): void };
```

- `PlayMode`（**概念**，ADR-0011/0012，变体驼峰 `SinglePlay`/`AiGuide`）= 领域概念「一个 Game 从哪个视角展示」，
  只在**前端组装层**有意义——后端**无对应类型**（单 `Game`、无模式字段；「模式」只隐式体现在端点划分
  `/state`/`/action` 单局 vs `/ai/guide/:id` guide 流）。
- `PlayModeName`（**前端代码值**，kebab `"single"`/`"ai-guide"`）= `PlayMode` 概念的**运行时标识**，供
  `mountMode`/`renderModeSwitcher` 判断；**UI 显示名**用驼峰 `SinglePlay`/`AiGuide`（属 label，非 `PlayModeName`）。
  切 mode = 前端弃局开新局，对后端透明。
- 每个 mode 独立 DOM + 独立 `createGameClient`（ADR-0012）。
- **定案**：`AppDeps` 注入 `aiApi` + `captureBoardImage`（B2）。`captureBoardImage` 是浏览器真实截图
  （html-to-image），测试环境（jsdom）无法真截图，须依赖注入以便 stub；`createBoardAxis`（轴标）是
  纯 DOM、测试可跑，直接 import，不进 `AppDeps`。
- **定案**：仪表盘预留「session 策略」下拉（`SessionStrategy`：`per-analysis` 可用 / `per-game` 标注
  「(未实现)」置灰不可选；不与 `BoardFormat` 混）。本 map 只实现 `per-analysis`（每分析全新上下文）。
  **切换 session 策略 = 弃局开新局 + 清分析**（同切 mode 语义）。`per-game` 为将来「同 session 多次分析
  拼接上下文」留口；**本期不传入核心 seam**（`AiApi`/`AnalysisMachine`/`Guide` 签名不变）。

---

## Seam 汇总（供审阅）

| # | seam（模块边界） | 性质 | 主要 pub 接口 |
|---|---|---|---|
| S1 | `core::Game` | 复用，无新增 | 可见 API（只读） |
| S2 | `ai::protocol` | 共享契约 | `Message`, `ChatRequest`, `StreamChunk`, `ProviderError` |
| S3 | `ai::provider` | 访问机制 | `Provider::stream_chat`, `ProviderStream`, `DeepSeek` |
| S4 | `ai::agent` | 定义 | `Tool`, `Session`, `Agent::{stream,complete_once,run_loop}`, `StreamChunk`, `AgentError` |
| S5 | `ai_adapter` | 绑定 | `BoardView`, `system_prompt`, `build_text_blocks/build_image_blocks`, `Guide::suggest`（经 `Agent`）, `StreamChunk`, `InterruptReason`, `SuggestPreFlightError`, `GuideRequest` |
| S6 | `server` 传输 | 薄传输 | `ai_routes`, `GuideEventDto`, `ai::protocol::ProviderError`（错误体） |
| S7 | `ai/api.ts` | wire | `AiApi`, `GuideEvent`, `GuideRequest` |
| S8 | `ai/stateMachine.ts` | 状态机 | `AnalysisMachine`, `AnalysisState` |
| S9 | `ai/conversation.ts` | 渲染 | `createConversation` |
| S10 | `ai/axis.ts` | 渲染 | `createBoardAxis` |
| S11 | `ai/screenshot.ts` | 工具 | `captureBoardImage` |
| S12 | `app/` 组装 | 组合 | `mountMode`, `renderModeSwitcher`, `composeGuideMode` |

## 交叉切面：隐私与 promise

- **保密性**：`ai_adapter::BoardView::from_game` 是唯一允许读取 `core::Game` 可见侧的地方；`Guide::suggest`/`build_*` 只消费
  `BoardView`。`mines` 在 `core` 内且 `#[cfg(test)]`，任何 `ai`/`ai_adapter`/`server` 路径都触不到。
  **测试断言**：发给 mock `Provider` 的 `ChatRequest.messages` 不含任何 mine 位置/布局信息。
- **单流、单终止**：#97 终止统一由后端 SSE 终止 event 裁决；`ai::provider` 只产生 `Done` 或（中断时）流结束，
  `ai_adapter` 流中 `Err` → `Interrupt(reason)`，`server` 原样转发，前端 `ai/stateMachine.ts` 据 `interrupt` 渲染红字。
