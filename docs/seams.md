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

### S2 `ai::provider`（主 seam）—— `Provider` trait + `deepseek` 实现

AI「大脑」的单点。`ai_adapter::Guide::suggest` 与未来 `ai_play` 都经它；测它时注入 mock `Provider`。

**文件划分（目录 mod，非平铺）**：`src/ai/provider/` 下 `mod.rs` 是抽象 seam（`Provider` trait + 共享类型），
`deepseek.rs` 是具体实现。二者**都**对外 `pub`（`pub mod deepseek`）：`main` 作为组合根要读 `DEEPSEEK_API_KEY`、
选 provider、把 key 注入 `DeepSeek`；`ai_adapter`/`Guide` 只消费 `Box<dyn Provider>`。抽象 seam 不反向依赖 `deepseek`。

**实现注记**：本 spec 阶段 `main` 先 **hardcode `DeepSeek`**（直接 `DeepSeek::new` 注入 key），**不**实现
`--provider`/`--model` CLI 选择——那只是为将来留口。但构造点收在 `main`、上层只见 `Box<dyn Provider>`，将来
要加 CLI 时只需在 `main` 的 provider 选择处扩展，`ai_adapter`/`Guide`/S3`Agent` 零改动。

```rust
// ai::provider/mod.rs（抽象 seam；具体实现见 deepseek.rs）
use futures::Stream;

/// 模型就是 provider 特有的「名字」字符串——不写死 enum（供应商会一直出新品，enum 会变成我们的维护负担）。
/// `--model` 直接把字符串照搬传入 provider；是否属于该 provider 由 provider 端（DeepSeek）校验。
/// DeepSeek 支持列表可经 `GET /models` 查询（可选 capability：校验模型名 / 填充选择器下拉）。

pub enum MessageRole { System, User, Assistant }
pub enum ContentPart {
    Text(String),
    /// data URL / base64 的 PNG；仅 vision-exp，单图 ≤384 token（#92）。<=128k no
    ImageUrl(String),
}
pub struct Message { pub role: MessageRole, pub content: Vec<ContentPart> }

/// 工具（未来 AiPlay 用；顾问模式传空集）
pub struct ToolSpec {
    pub name: String, pub description: String,
    pub parameters: serde_json::Value,   // JSON Schema
}

pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: String,          // 具体模型名（"deepseek-v4-flash" 等），透传给 provider 校验
    pub stream: bool,           // 顾问恒 true（SSE）
    pub tools: Vec<ToolSpec>,   // 顾问空集
}

/// 流式过程中的一个事件（content 与 reasoning_content 分开 —— #95 双流）。
pub enum ProviderEvent {
    ReasoningDelta(String),
    ContentDelta(String),
    Done,                       // 终止、正常收尾（[DONE]）
}

/// 前置失败（未流任何内容之前）—— 映射到 #97 分桶。
pub enum ProviderError {
    Status { code: u16, body: String },   // 400/401/402/422 → 配置错误
    RateLimited,                          // 429 → 上游瞬时
    Upstream,                             // 500/503 → 上游瞬时
    Network,
    Timeout,
}

/// 上游生成是否已被取消（用户中断 / 超时 / 上游错误），由调用侧 `select!` 驱动。
// 类型由 tokio-util 提供；此处引用。
use tokio_util::sync::CancellationToken;

pub type ProviderStream = Pin<Box<dyn Stream<Item = ProviderEvent> + Send>>;

pub trait Provider: Send + Sync {
    /// 前置失败返回 `Err`（映射 #97 ①）；否则返回 delta 流，末尾是 `Done`。
    /// 生成期间可通过 `cancel` 取消（用户中断 #97）——取消表现为流提前 error/结束，
    /// 由 `ai_adapter::Guide::suggest` 映射为 `Terminated(user_interrupt)`。
    async fn stream_chat(
        &self, req: ChatRequest, cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError>;
}
```

```rust
// provider/deepseek.rs —— 具体实现（pub，供 main 构造/选择；抽象 seam 不引用它）
pub struct DeepSeek { api_key: String, base_url: String, client: reqwest::Client }
pub struct DeepSeekConfig { pub api_key: String, pub base_url: String }
impl DeepSeek {
    /// 读 env `DEEPSEEK_API_KEY` 构造；无 key 时说明 AI 不启用。
    pub fn new(config: DeepSeekConfig) -> Self;
    /// `GET /models` 查询本 provider 支持的模型（可选 capability：校验模型名 / 填充选择器）。
    pub async fn list_models(&self) -> Result<Vec<String>, ProviderError>;
    /// 校验模型名是否属于本 provider；不属于则 Err（供 `--model` 前置校验）。
    pub fn validate_model(&self, model: &str) -> Result<(), ProviderError>;
}
impl Provider for DeepSeek {
    // model 字符串直接作为 API 的 model 参数；无 enum、无我们维护的字符串映射表。
}
```

- 依赖新增：`futures`（Stream）、`tokio-util`（CancellationToken）、`reqwest`（deepseek 实现）。
- **待确认**：`ProviderError` 要不要细分成「配置类」/「上游类」两个更上层的 kind，以便 server 直接映射
  `AiErrorKind`？目前用 `Status{code}` 的 code 分支判断，你若倾向显式 kind，我可加一个
  `ProviderError::kind() -> ProviderErrorKind`。

### S3 `ai::agent` —— `Tool` / `Session` / `Agent`（run_loop）

ADR-0013 的 `agent → provider`。顾问用 `complete_once`（单轮、无工具）；`run_loop`/`Tool` 为未来 `AiPlay` 预留，但**接口一并定义**（按你的决定）。

```rust
// ai::agent
use crate::provider::{Message, Provider};

/// 一个工具（未来 AiPlay；顾问空集）。
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value, String>;
}

/// 一轮对话的增量历史（只追加）。
pub struct Session { /* Vec<Message> */ }
impl Session {
    pub fn new(system: Message) -> Self;          // role: System
    pub fn push(&mut self, msg: Message);
    pub fn messages(&self) -> &[Message];
}

pub struct Agent { /* provider, model, Arc<Vec<Tool>> */ }
impl Agent {
    pub fn new(provider: Box<dyn Provider>, model: String) -> Self;
    pub fn add_tool(&mut self, tool: Arc<dyn Tool>);          // 预留给 AiPlay
    /// 单轮完成（无工具循环）—— 顾问路径。
    /// 也是 dev 手工测试 `--test-ai-chat` 的入口（聚合、非流式；无需 `Provider` 新增接口）。
    pub async fn complete_once(
        &self, session: &Session, cancel: CancellationToken,
    ) -> Result<AgentReply, AgentError>;
    /// 持久工具循环 —— 未来 AiPlay；本 map 不用。
    pub async fn run_loop(
        &self, session: &Session, cancel: CancellationToken,
    ) -> Result<AgentReply, AgentError>;
}

pub struct AgentReply {
    pub content: String,
    pub reasoning: Vec<String>,     // reasoning_content 各 delta
}
pub enum AgentError {
    Provider(ProviderError),
    Cancelled,
}
```

- **待确认**：`complete_once` 返回的是**整个** `AgentReply`（消费完整流），而要流式给前端，`ai_adapter::Guide::suggest` 会直接消费 `Provider` 的流并转发，而非走 `complete_once`。两者并存是否 OK？
  （即：`complete_once` 是「聚合结果」的工具/后端测 seam；`suggest` 是「流式」的对外 seam。可在 S4 里让 `suggest` 复用 `complete_once` 的内部，但对外仍流式。）

### S4 `ai_adapter` —— `BoardFormat` / `BoardView` / `system_prompt` / `render_*` / `Guide::suggest`

扫雷绑定：把 `core::Game` 的**可见**侧渲染成 #94 4 形式 + 拼 system prompt；`Guide::suggest` 是用户「点问 AI」的入口。

```rust
// ai_adapter（依赖 core + ai；不依赖 server）
use crate::core::{Game, CellView, Difficulty, GameState, Position};
use crate::ai::provider::{ContentPart, Message, Provider, ProviderError};
use crate::ai::agent::Tool;

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
pub fn render_text(view: &BoardView, format: BoardFormat) -> Vec<ContentPart>;
/// 图像形式（D）的 user 消息体 = 头部文本 + 截图 data URL。纯。
pub fn render_image(view: &BoardView, image_data_url: &str) -> Vec<ContentPart>;

/// 流式转发给前端的事件（reasoning / content 分开，结束时给终止 event）。
pub enum GuideEvent {
    ReasoningDelta(String),
    ContentDelta(String),
    Done,                          // [DONE] 正常收尾
    Terminated(InterruptReason),   // 未收 [DONE] —— #97 ②
}

/// 终止原因（#97）。wire / 前端镜像此值。
pub enum InterruptReason { UserInterrupt, RateLimit, Timeout, UpstreamError, Unknown }

/// 前置失败 → 分桶（#97 ①），走 HTTP 状态码 + 结构化错误体。
pub enum SuggestError { PreFlight(ProviderError) }

/// 请求：前端只发 format / model（+ 图像形式的 image）。文本形式的棋盘由后端读自己的 Game。
pub struct GuideRequest {
    pub format: BoardFormat,
    pub model: String,            // 具体模型名（透传给 provider）；前端不校验
    pub image: Option<String>,    // image 形式：前端 `html-to-image` 的 PNG data URL
}

/// 一次性顾问：注入棋盘、走一轮、流式返回，末尾终止 event。
/// 前置失败 → Err(PreFlight)；否则返回事件流（delta…+ Done | Terminated）。
pub struct Guide { provider: Box<dyn Provider>, model: String }
impl Guide {
    /// 用 provider（DeepSeek 或 mock）+ 默认 model 构造 Guide。
    pub fn new(provider: Box<dyn Provider>, model: String) -> Self;
    /// 注入棋盘、走一轮、流式返回；`cancel` 可中断上游生成（#97）。
    pub async fn suggest(
        &self, game: &Game, req: GuideRequest, cancel: CancellationToken,
    ) -> Result<impl Stream<Item = GuideEvent>, SuggestError>;
}

/// 工具绑定（未来 AiPlay；顾问传空集）。绑到 GameHandle 以便未来双 Game 不换 adapter。
pub fn tools(handle: &GameHandle) -> Vec<Arc<dyn Tool>>;
```

- **隐私**：`BoardView::from_game` 是隐私边界——只调 `cell_view`/`game_state`/`difficulty`/`size`/
  `flags_remaining`，**不**读 `mines`。测它时断言 payload 不含任何 mine 位置。
- **待确认**：
  - 文本形式**由后端读自己的 `Game` 渲染**、前端只发 `format`/`model`（+`image`）——这修正了 spec 第 10 节
    「请求体含玩家可见棋盘」的字面。**是否接受**？（我倾向如此：后端 Game 权威、前端薄、隐私收紧在后端。）
  - `SUGGEST {"row":N,"col":M}` **不做任何解析**（#95「永不解析、人读文本」）。事件流里**没有**坐标字段；
    `SUGGEST`/`SUGGEST null` 只是 content 末尾一段文本。**确认**不需要后端「报告」解析出的坐标。
  - `GuideRequest` 里要不要暴露 `model` 选择？还是后端配置默认、前端不带？（#96 未提模型选择器。）

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

/// 前置失败的错误体（HTTP 状态码 + 结构化 body → #97 ① alert 分桶）。
pub struct AiErrorBody {
    pub kind: AiErrorKind,     // config | upstream
    pub code: u16,
    pub message: String,
}
pub enum AiErrorKind { Config, Upstream }

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

export type PreFlightError = { kind: "config" | "upstream"; code: number; message: string };

export interface GuideRequest { format: BoardFormat; model: string; image?: string; }

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
| S2 | `ai::provider::Provider` | **主 seam** | `stream_chat`, `ChatRequest`, `ProviderEvent`, `ProviderError` |
| S3 | `ai::agent` | 定义 | `Tool`, `Session`, `Agent::{complete_once,run_loop}` |
| S4 | `ai_adapter` | 绑定 | `BoardView`, `system_prompt`, `render_text/image`, `Guide::suggest`, `GuideEvent`, `InterruptReason`, `GuideRequest` |
| S5 | `server` 传输 | 薄传输 | `ai_routes`, `AiStreamEvent`, `AiErrorBody` |
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

1. **S4 请求体**：文本形式由后端读自己的 `Game` 渲染、前端只发 `format`/`model`（+`image`）——接受？（修正 spec §10 字面。）
2. **`SUGGEST` 永不解析**：确认事件流/结果里**不**带解析出的坐标字段。
3. **`model` 暴露**：前端是否可选模型，还是后端默认？（#96 没提模型选择器。）
4. **分析 `id` 归属**：S5 的 `/ai/guide/:id/interrupt` 的 `<id>`，前端生成 vs 后端分配？
5. **S5 `AiStreamEvent` 形状**：带 `kind` 的联合（`reasoning/content/done/terminated`）是否就是你要的 wire 形状？
6. **S3 vs S4**：`complete_once`（聚合）与 `suggest`（流式）并存——OK？
7. **S2 `ProviderError`**：是否加显式的 `kind()`（config/upstream）以便 S5 直接映射 `AiErrorKind`？
