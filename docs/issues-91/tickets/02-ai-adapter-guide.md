# 02: ai_adapter 扫雷绑定（BoardFormat / BoardView / Guide::suggest）

**What to build:** 把扫雷 `Game` 的**玩家可见**侧渲染成 #94 的 4 种输入形式 + 共享 system prompt；`Guide::suggest` 注入一盘、走一轮（经 mock Provider）返回分析流。**隐私硬约束**：发送的 payload 只含玩家可见状态，**绝不泄露 Mine 布局**。本 ticket 不接真实 DeepSeek HTTP。


**Blocked by:** 01

**Status:** ready-for-agent

- [ ] `BoardView::from_game` 只读 `cell_view`/`game_state`/`difficulty`/`size`/`flags_remaining`，不读 `mines`；任何发给 mock Provider 的 `ChatRequest.messages` 断言不含任何 mine 位置/布局。
- [ ] 4 种形式（simple-text / emoji / full-coordinates / image）都能从同一个 `BoardView` 产出，头部含 `Difficulty`/`Rows`/`Cols`/`Mine count`/`Flags remaining`/`Game state`；`Mine count` 为 recipe 常量、`flags_remaining` 可为负（over-flag）。
- [ ] 共享 `system_prompt`：坐标 0-based、只看玩家可见、末尾 `SUGGEST` 契约（`{"row":N,"col":M}` / `null`）；各形式天然复用同一 prompt。
- [ ] `Guide::suggest(&self, game, req: GuideRequest, cancel)` 经 mock Provider 走一轮：`Ok(StreamChunk)`（reasoning/content delta）推进、`Ok(Done)` 收尾；流中中断 `Err(InterruptReason)`；前置失败 `Err(SuggestPreFlightError)`。
- [ ] `BoardFormat` wire 序列化 = kebab-case（`simple-text`/`emoji`/`full-coordinates`/`image`），且可 `Deserialize`（来自前端请求体）；`GuideRequest` 只带 `format` + 可选 `image_data_url`，**不带 model**（后端 DeepSeek 默认）。

### 接口契约

```rust
// 依赖 core::Game 可见 API（只读）：game_state() / difficulty() / size() / flags_remaining() / cell_view(pos) -> CellView

#[derive(serde::Serialize, serde::Deserialize)] #[serde(rename_all = "kebab-case")]
enum BoardFormat { SimpleText, Emoji, FullCoordinates, Image }

struct BoardView { difficulty: Difficulty, state: GameState, rows: usize, cols: usize,
                   mine_count: usize, flags_remaining: i32, cells: Vec<CellView> }   // 不含 mine 布局（隐私硬约束）
impl BoardView { fn from_game(game: &Game) -> Self; }   // 隐私 seam：只读可见侧
// S1 定案：**不**给 core::Game 新增 snapshot() 等可见方法——cell_view + flags_remaining + difficulty().mine_count() 已够拼出头部与棋盘

fn system_prompt() -> String;   // 坐标 0-based、只看可见、末尾 SUGGEST 契约
fn build_text_blocks(view: &BoardView, format: BoardFormat) -> Vec<ContentBlock>;
fn build_image_blocks(view: &BoardView, image_data_url: &str) -> Vec<ContentBlock>;

enum InterruptReason { UserInterrupt, RateLimit, Timeout, UpstreamError, Unknown }
struct SuggestPreFlightError(AgentError);
struct GuideRequest { format: BoardFormat, image_data_url: Option<String> }   // 不带 model
const DEFAULT_MODEL: &str = "deepseek-v4-flash";
const VISION_MODEL: &str = "deepseek-v4-flash-vision-exp";

struct Guide { agent: Arc<Mutex<Agent>> }
impl Guide {
  fn new(agent: Arc<Mutex<Agent>>) -> Self;
  async fn suggest(&self, game: &Game, req: GuideRequest, cancel: CancellationToken)
      -> Result<impl Stream<Item = Result<StreamChunk, InterruptReason>>, SuggestPreFlightError>;
  // 内部：lock() 拿 &mut Agent → format==Image ? set_model(VISION_MODEL, None) : set_model(DEFAULT_MODEL, None)
  //   image 形式先把 req.image_data_url 留底写盘 <exe_dir>/base64_img/YYYYMMDD_<seed>_<seq>.png（失败不阻断）
  //   build 棋盘消息 → Session → agent.stream() 透传；Err(AgentError::Cancelled)→Interrupt(UserInterrupt)
  //   Err(AgentError::Provider(pe))→Interrupt(rate_limit/timeout/upstream)；无并发需求（同刻仅一个 running suggest）
}

struct GameHandle { /* 封装 Arc<Mutex<core::Game>>；TODO AiPlay 落地时补 */ }
fn tools(handle: &GameHandle) -> Vec<Arc<dyn Tool>>;   // 顾问空集
