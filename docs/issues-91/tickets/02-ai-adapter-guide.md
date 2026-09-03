# 02: ai_adapter 扫雷绑定（BoardFormat / BoardView / Guide::suggest）

**What to build:** 把扫雷 `Game` 的**玩家可见**侧渲染成 #94 的 4 种输入形式 + 共享 system prompt；`Guide::suggest` 注入一盘、走一轮（经 mock Provider）返回分析流。**隐私硬约束**：发送的 payload 只含玩家可见状态，**绝不泄露 Mine 布局**。本 ticket 不接真实 DeepSeek HTTP（留 03）。

**Coverage seams:** S5（+ 读 S1 `core::Game` 可见 API）

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] `BoardView::from_game` 只读 `cell_view`/`game_state`/`difficulty`/`size`/`flags_remaining`，不读 `mines`；任何发给 mock Provider 的 `ChatRequest.messages` 断言不含任何 mine 位置/布局。
- [ ] 4 种形式（simple-text / emoji / full-coordinates / image）都能从同一个 `BoardView` 产出，头部含 `Difficulty`/`Rows`/`Cols`/`Mine count`/`Flags remaining`/`Game state`；`Mine count` 为 recipe 常量、`flags_remaining` 可为负（over-flag）。
- [ ] 共享 `system_prompt`：坐标 0-based、只看玩家可见、末尾 `SUGGEST` 契约（`{"row":N,"col":M}` / `null`）；各形式天然复用同一 prompt。
- [ ] `Guide::suggest(&self, game, req: GuideRequest, cancel)` 经 mock Provider 走一轮：`Ok(StreamChunk)`（reasoning/content delta）推进、`Ok(Done)` 收尾；流中中断 `Err(InterruptReason)`；前置失败 `Err(SuggestPreFlightError)`。
- [ ] `BoardFormat` wire 序列化 = kebab-case（`simple-text`/`emoji`/`full-coordinates`/`image`），且可 `Deserialize`（来自前端请求体）；`GuideRequest` 只带 `format` + 可选 `image_data_url`，**不带 model**（后端 DeepSeek 默认）。
