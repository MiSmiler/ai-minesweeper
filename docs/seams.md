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

### S2（主 seam）—— `ai::protocol`（共享协议值） + `ai::provider`（`Provider` trait + `deepseek`）

AI「大脑」的单点。`ai_adapter::Guide::suggest` 与未来 `ai_play` 都经它；测它时注入 mock `Provider`。

**文件划分（目录 mod，非平铺）**：
- `src/ai/protocol/` —— **共享协议/值类型**（与供应商解耦）：`Message`/`ContentBlock`/`ToolCall`/`ToolDecl`/
  `ChatRequest`/`ProviderEvent`/`ProviderError`。`provider`/`agent`/`ai_adapter`/`server` 都引用。
- `src/ai/provider/` —— **访问供应商的机制**：`Provider` trait（`stream_chat`）+ `deepseek.rs`（`DeepSeek`
  实现，持 `api_key`/`base_url`/`client`/`model`）。二者**都**对外 `pub`（`pub mod deepseek`）：`main` 作为
  组合根读 `DEEPSEEK_API_KEY`、选 provider、注入 key 与默认 model；`ai_adapter`/`Guide` 只面向 `Agent`、
  不再直连 `Provider`。抽象 seam 不反向依赖 `deepseek`。

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

/// 流式过程中的一个事件（content 与 reasoning_content 分开 —— #95 双流）。
pub enum ProviderEvent {
    ReasoningDelta(String),
    ContentDelta(String),
    Done,                       // 终止、正常收尾（[DONE]）
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

```rust
// ai::provider/mod.rs —— 访问供应商的机制：Provider trait + ProviderStream
use crate::protocol::{ChatRequest, ProviderEvent, ProviderError};
use futures::Stream;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

/// 上游生成是否已被取消（用户中断 / 超时 / 上游错误），由调用侧 `select!` 驱动。
pub type ProviderStream = Pin<Box<dyn Stream<Item = ProviderEvent> + Send>>;

pub trait Provider: Send + Sync {
    /// 前置失败返回 `Err`（映射 #97 ①）：provider 端先 `validate_model(req.model)`（model 不属于它 → `Config`）。
    /// 通过后返回 delta 流，末尾是 `Done`。
    /// 生成期间可通过 `cancel` 取消（用户中断 #97）——取消表现为流提前 error/结束，
    /// 由 `ai_adapter::Guide::suggest` 映射为 `Terminated(user_interrupt)`。
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

### S3 `ai::agent` —— `Tool` / `Session` / `Agent`（run_loop）

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
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value, String>;
}

/// 一轮对话的增量历史（只追加）。
pub struct Session { /* Vec<Message> */ }
impl Session {
    pub fn new(system: Message) -> Self;          // Message::System（system prompt）
    pub fn push(&mut self, msg: Message);
    pub fn messages(&self) -> &[Message];
}

/// agent 层的流式事件（隔离 `ProviderEvent`；ai_adapter 只消费本类型）。
pub enum AgentEvent {
    ReasoningDelta(String),
    ContentDelta(String),
    Done,           // [DONE]
    Cancelled,      // cancel 触发（#97 ②）
}

pub enum AgentError {
    Provider(ProviderError),
    NoProvider,     // stream 时 current_provider 为空 / 容器无此 provider（未 set_model 或切错名）
    Cancelled,
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
    /// 流式：把 session 发出去（`ChatRequest.model` 填 `current_model`），边收边转 `AgentEvent`（隔离 `ProviderEvent`）。
    /// 内部 `providers.get(&self.current_provider)` 查不到 → `Err(NoProvider)`；其余前置失败 → `Err(Provider)`。
    /// 生成期间可由 `cancel` 中断（#97）。
    pub async fn stream(
        &self, session: &Session, cancel: CancellationToken,
    ) -> Result<impl Stream<Item = AgentEvent>, AgentError>;
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

### S4 `ai_adapter` —— `BoardFormat` / `BoardView` / `system_prompt` / `render_*` / `Guide::suggest`

扫雷绑定：把 `core::Game` 的**可见**侧渲染成 #94 4 形式 + 拼 system prompt；`Guide::suggest` 是用户「点问 AI」的入口。

```rust
// ai_adapter（依赖 core + ai；不依赖 server）
use crate::core::{Game, CellView, Difficulty, GameState, Position};
use crate::ai::protocol::{ContentBlock, Message};
use crate::ai::agent::{Agent, AgentError, Tool};

/// #94 4 种呈现形式。
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
pub fn render_text(view: &BoardView, format: BoardFormat) -> Vec<ContentBlock>;
/// 图像形式（D）的 user 消息体 = 头部文本 + 截图 data URL。纯。
pub fn render_image(view: &BoardView, image_data_url: &str) -> Vec<ContentBlock>;

/// 流式转发给前端的事件（reasoning / content 分开，结束时给终止 event）。
pub enum GuideEvent {
    ReasoningDelta(String),
    ContentDelta(String),
    Done,                          // [DONE] 正常收尾
    Terminated(InterruptReason),   // 未收 [DONE] —— #97 ②
}

/// 终止原因（#97）。wire / 前端镜像此值。
pub enum InterruptReason { UserInterrupt, RateLimit, Timeout, UpstreamError, Unknown }

/// 前置失败 → bucket（#97 ①），走 HTTP 状态码 + 结构化错误体。
pub enum SuggestError { PreFlight(AgentError) }

/// 请求：前端只发 format（+ 图像形式的 image）。文本形式的棋盘由后端读自己的 Game；**不带 model**（后端 `DeepSeek` 默认）。
pub struct GuideRequest {
    pub format: BoardFormat,
    pub image: Option<String>,    // image 形式：前端 `html-to-image` 的 PNG data URL
}

/// 一次性顾问：注入棋盘、走一轮、流式返回，末尾终止 event。
/// 前置失败 → Err(PreFlight)；否则返回事件流（delta…+ Done | Terminated）。
/// 多模态（vision）模型名——format D（Image）时切换（`agent.set_model(VISION_MODEL)`）。#92
const VISION_MODEL: &str = "deepseek-v4-flash-vision-exp";

/// 多线程共享：`Arc<Mutex<Agent>>`——`&self` 方法内 `lock()` 拿到 `&mut Agent` 就能 `set_model`（只改 model 记录）。
pub struct Guide { agent: Arc<Mutex<Agent>> }
impl Guide {
    /// 用共享 Agent（DeepSeek 或 mock 构造）构造 Guide。
    pub fn new(agent: Arc<Mutex<Agent>>) -> Self;
    /// 注入棋盘、走一轮、流式返回；`cancel` 可中断上游生成（#97）。
    pub async fn suggest(
        &self, game: &Game, req: GuideRequest, cancel: CancellationToken,
    ) -> Result<impl Stream<Item = GuideEvent>, SuggestError>;
    // 内部：let mut agent = self.agent.lock().unwrap();
    //       req.format == Image → agent.set_model(VISION_MODEL, None)（只改 model，provider 沿用当前；#92）
    //       render 棋盘 → Session(Message::System/User) → agent.stream()（agent 自动填 current_model）
    //       把 AgentEvent 映射成 GuideEvent（Done→Done、Cancelled→Terminated(reason)），
    //       把 AgentError 映射成 SuggestError。
    // 注：lock() 借 `&mut Agent` 调 set_model；stream 是 `&self`，同 guard 下可续用。
}

/// 工具绑定（未来 AiPlay；顾问传空集）。绑到 GameHandle 以便未来双 Game 不换 adapter。
pub fn tools(handle: &GameHandle) -> Vec<Arc<dyn Tool>>;
```

- **隐私**：`BoardView::from_game` 是隐私边界——只调 `cell_view`/`game_state`/`difficulty`/`size`/
  `flags_remaining`，**不**读 `mines`。测它时断言 payload 不含任何 mine 位置。
- **待确认**：
  - 文本形式**由后端读自己的 `Game` 渲染**、前端只发 `format`（+`image`），**不带 `model`**——这修正了 spec 第 10 节
    「请求体含玩家可见棋盘」的字面。**是否接受**？（我倾向如此：后端 Game 权威、前端薄、隐私收紧在后端。）
  - `SUGGEST {"row":N,"col":M}` **不做任何解析**（#95「永不解析、人读文本」）。事件流里**没有**坐标字段；
    `SUGGEST`/`SUGGEST null` 只是 content 末尾一段文本。**确认**不需要后端「报告」解析出的坐标。
  - `model` 暴露：**定案**——前端不带，后端 `DeepSeek` 默认（`GuideRequest.model` 删；`#96` 未提模型选择器，留 `--model` CLI 口）。

### S5 `server` —— `/ai/...` 传输 seam

薄传输层：把 `ai_adapter::Guide::suggest` 的事件流 SSH 转发给前端，并处理中断/终止。

```rust
// server
pub fn ai_routes(state: Arc<AppState>) -> Router;
//  POST /ai/guide/:id        → SSE 流（AiStreamEvent…，收 [DONE] 结束）
//  POST /ai/guide/:id/interrupt → 取消上游，驱动同 SSE 的 {reason:"user_interrupt"}

/// SSE 流上的 wire 事件（镜像 ai_adapter::GuideEvent + 终止原因）。
pub enum AiStreamEvent {
    Reasoning(String),
    Content(String),
    Terminated { reason: InterruptReason },   // 末端终止 event（#97）
}
// 注：[DONE] 是 SSE 流的收官标记；`Terminated` 是「未收 [DONE]」时的显式终止。

/// 前置失败的错误体 = `ai::protocol::ProviderError`（带 `Serialize`，序列化为 { kind, code, message }）。
/// 不再单独定义 wire 错误体；`server` 直接把 `ProviderError` 序列化返回给前端。

/// 路由处理逻辑（与现在测 apply_action 同思路，可独立测）。
pub(crate) fn handle_guide(...) -> impl IntoResponse;   // SSE
pub(crate) fn handle_interrupt(...) -> impl IntoResponse;
```

- **待确认**：分析 `id` 由谁生成？`/ai/guide/:id/interrupt` 需要 `<id>`。我倾向**前端**（client）生成
  id（如 `crypto.randomUUID()`），因为「中断」由前端发起、且前端持有该分析会话；`id` 随 SSE 建立即锁定。
  **若你倾向后端分配（在 SSE 首帧下发 id），请指出。**
- **待确认**：`AiStreamEvent` 要不要合并 `reasoning`/`content` 为一个带 `kind` 的事件，前端好接？我按你
  前端状态机偏好，倾向**带 kind**（见 S8 `AiEvent`），此处与 wire 保持同构。

---

## 前端 seams

### S6 `app/` 组装 seam（Mode switcher + `PlayMode` 组合）

`app/` 是 mode 组合处（ADR-0011/0012）。guide 组合拿 `game/` slice + `ai/` slice 拼。

```ts
// app/
export type PlayModeName = "single" | "ai-guide";

export interface AppDeps {
  getPlayMode(): PlayModeName;
  // 复用 game slice 的既有入口（createGameClient 由各 mode 实例化）
  // ai/ slice 的入口（由 composeGuideMode 用）
  aiApi: AiApi;
}

/** 挂载某个 mode 的组合，返回卸载函数。切 mode 即调用当前卸载 + 挂载新的。 */
export function mountMode(mode: PlayModeName, root: HTMLElement, deps: AppDeps): () => void;

/** 顶栏 mode-switcher；切 mode 触发 onSwitch。 */
export function renderModeSwitcher(root: HTMLElement, current: PlayModeName, onSwitch: (m: PlayModeName) => void): void;

/** 组装 guide 模式（独立 DOM + 独立 game client + ai/ slice）。 */
export function composeGuideMode(root: HTMLElement, deps: AppDeps): { dispose(): void };
```

- 切 mode = 弃局开新局（后端仍是单 `Game`）；每个 mode 独立 DOM + 独立 `createGameClient`（ADR-0012）。
- **待确认**：`AppDeps` 里 `aiApi` 要不要进一步拆成 `AiApi` + `captureBoardImage` + 轴标组件工厂，
  便于 assemble 时按需注入、测试时 mock 更细？

### S7 `ai/api.ts` —— wire 契约（镜像后端 `/ai/...`）

```ts
// ai/api.ts
export type BoardFormat = "simple-text" | "emoji" | "full-coordinates" | "image";
// model 是 provider 特有的名字字符串（"deepseek-v4-flash" 等），不写死 union。
export type InterruptReason =
  | "user_interrupt" | "rate_limit" | "timeout" | "upstream_error" | "unknown";

export type AiEvent =
  | { kind: "reasoning"; text: string }
  | { kind: "content"; text: string }
  | { kind: "done" }
  | { kind: "terminated"; reason: InterruptReason };

export type PreFlightError = { kind: "config" | "upstream"; code: number | null; message: string };

export interface GuideRequest { format: BoardFormat; image?: string; }   // 不带 model（后端 DeepSeek 默认）

export interface AiApi {
  /** POST /ai/guide/:id —— 消费 SSE 流，逐 event 回调；前置失败走 onPreFlightError。 */
  startGuide(id: string, req: GuideRequest, onEvent: (e: AiEvent) => void, onPreFlightError: (e: PreFlightError) => void): void;
  /** POST /ai/guide/:id/interrupt。 */
  interrupt(id: string): Promise<unknown>;
}
```

- 前端**不解析** `SUGGEST`（#95）；`AiEvent` 只有 reasoning/content/done/terminated，**没有坐标字段**。
  image 形式的 base64 由 S11 收集后放进 `GuideRequest.image`。

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
  preFlightError?: PreFlightError;     // preflight-failed 时
}

export interface AnalysisMachine {
  /** 发起一轮分析；每轮 = 最新棋盘（不缓存失败快照，#97）。 */
  start(req: GuideRequest): void;
  /** 用户中断：POST interrupt（前端保持 SSE 不 abort，#97）。 */
  interrupt(): Promise<void>;
  /** 输入格式变更 / 新局 / 切 mode 时清空（历史清除语义，见 S6 组装）。 */
  reset(): void;
  /** 订阅状态变化，返回退订。 */
  onState(cb: (s: AnalysisState) => void): () => void;
}

export function createAnalysisMachine(deps: { api: AiApi; newId: () => string }): AnalysisMachine;
```

- 历史绑定一局、分析中不可点、输入格式变更 → 确认 + 清历史（#96）—— 这些判定放组装层（S6），状态机只负责
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
- 供 S7 `GuideRequest.image` 用；Playwright 只作开发/工具截图，不作 runtime capture。

---

## Seam 汇总（供审阅）

| # | seam（模块边界） | 性质 | 主要 pub 接口 |
|---|---|---|---|
| S1 | `core::Game` | 复用，无新增 | 可见 API（只读） |
| S2 | `ai::protocol` + `ai::provider` | **主 seam** | `Message`/`ChatRequest`/`ProviderEvent`（protocol）；`Provider::stream_chat`, `ProviderStream`（provider） |
| S3 | `ai::agent` | 定义 | `Tool`, `Session`, `Agent::{stream,complete_once,run_loop}`, `AgentEvent` |
| S4 | `ai_adapter` | 绑定 | `BoardView`, `system_prompt`, `render_text/image`, `Guide::suggest`（经 `Agent`）, `GuideEvent`, `InterruptReason`, `SuggestError`, `GuideRequest` |
| S5 | `server` 传输 | 薄传输 | `ai_routes`, `AiStreamEvent`, `ai::protocol::ProviderError`（错误体） |
| S6 | `app/` 组装 | 组合 | `mountMode`, `renderModeSwitcher`, `composeGuideMode` |
| S7 | `ai/api.ts` | wire | `AiApi`, `AiEvent`, `GuideRequest` |
| S8 | `ai/stateMachine.ts` | 状态机 | `AnalysisMachine`, `AnalysisState` |
| S9 | `ai/conversation.ts` | 渲染 | `createConversation` |
| S10 | `ai/axis.ts` | 渲染 | `createBoardAxis` |
| S11 | `ai/screenshot.ts` | 工具 | `captureBoardImage` |

## 交叉切面：隐私与 promise

- **保密性**：S4 `BoardView::from_game` 是唯一允许读取 `core::Game` 可见侧的地方；`Guide::suggest`/`render_*` 只消费
  `BoardView`。`mines` 在 `core` 内且 `#[cfg(test)]`，任何 `ai`/`ai_adapter`/`server` 路径都触不到。
  **测试断言**：发给 mock `Provider` 的 `ChatRequest.messages` 不含任何 mine 位置/布局信息。
- **单流、单终止**：#97 终止统一由后端 SSE 终止 event 裁决；S2 只产生 `Done` 或（中断时）流结束，
  S4 映射为 `GuideEvent::Terminated(reason)`，S5 原样转发，前端 S8 据 `terminated` 渲染红字。

## 仍需你拍板的清单（汇总）

1. **S4 请求体**：文本形式由后端读自己的 `Game` 渲染、前端只发 `format`（+`image`）、**不带 `model`**——接受？（修正 spec §10 字面。）
2. **`SUGGEST` 永不解析**：确认事件流/结果里**不**带解析出的坐标字段。
3. **`model` 暴露**：**定案**——前端不带，后端 `DeepSeek` 默认（`GuideRequest.model` 删；`#96` 未提模型选择器，留 `--model` CLI 口）。
4. **分析 `id` 归属**：S5 的 `/ai/guide/:id/interrupt` 的 `<id>`，前端生成 vs 后端分配？
5. **S5 `AiStreamEvent` 形状**：带 `kind` 的联合（`reasoning/content/done/terminated`）是否就是你要的 wire 形状？
6. **S3 vs S4**：`complete_once`（聚合）与 `suggest`（流式）并存——OK？
7. **S2 `ProviderError`**：已定——`ProviderError` 为通用载体（`kind` / `code: Option<u16>` / `message`）且 `serde::Serialize`，
  直接作为发送前端的 wire 错误体（删除了 `AiErrorBody` 和 `AiErrorKind`）；`ProviderErrorKind::{Config,Upstream}` 由 `deepseek::parse_http_error` 判定。
