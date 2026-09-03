# 04: server /ai/guide 传输路由与 SSE 终止

**What to build:** `POST /ai/guide/:id` 返回 SSE 流（`GuideEventDto`：`reasoning`/`content`/`interrupt`），`[DONE]` 收尾；`POST /ai/guide/:id/interrupt` 取消上游生成；前置失败返回 `ProviderError` 序列化错误体；`sessionId` 由前端生成、`server` 用它关联 cancel。**顺带在 `main.rs` 组合根完成 AI 组装**：读 `DEEPSEEK_API_KEY`、构造 `DeepSeek` → `ProviderSet` → `Agent`（`set_model`），并把 `ai_routes` 挂到 `Router`。


**Blocked by:** 02, 03

**Status:** ready-for-agent

- [ ] `POST /ai/guide/:id` 消费 `Guide::suggest` 流：`Ok(delta)` → data；`Ok(Done)` → 发 `[DONE]`；`Err(reason)` → `{kind:"interrupt",reason}` event；`GuideEventDto` 带 `kind`（`#[serde(tag="kind")]`）。
- [ ] `POST /ai/guide/:id/interrupt` 用同 `sessionId` 取消上游生成（`CancellationToken`），并在**同一 SSE** 发 `{kind:"interrupt",reason:"user_interrupt"}`；`rate_limit`/`timeout`/`upstream_error` 同理映射。
- [ ] 前置失败（HTTP 4xx/5xx、未流任何内容）→ 返回 `ProviderError` 序列化体 `{kind,code,message}`（`config`/`upstream`），不下发 SSE。
- [ ] `server` 是薄传输层：`ai_routes` 挂到组合根；`server` 不依赖 `ai_adapter` 内部实现（`handle_guide`/`handle_user_interrupt` 可独立测）。
- [ ] 一个 `Game` 同时只有一局；`/ai/...` 只读棋盘（不写 `Game`）。

### 接口契约

```rust
// main.rs（组合根）—— 读 DEEPSEEK_API_KEY → DeepSeek::new(DeepSeekConfig{api_key, base_url}) → ProviderSet.insert("deepseek", …) →
//   Agent::new(set) → agent.set_model(default_model, Some("deepseek")) → 挂 router（ai_routes）+ --test-ai-chat 早退分支

// server
fn ai_routes(state: Arc<AppState>) -> Router;
//  POST /ai/guide/:id          → SSE 流（GuideEventDto…，收 [DONE] 结束）
//  POST /ai/guide/:id/interrupt→ 取消上游，驱动同 SSE 的 {reason:"user_interrupt"}

#[serde(tag = "kind")]
enum GuideEventDto {
  Reasoning(String),          // → { kind:"reasoning", text }
  Content(String),            // → { kind:"content", text }
  Interrupt { reason: InterruptReason },   // 末端中断 event（#97）
}
// [DONE] 是 SSE 流收官标记（wire 无 Done 变体）；Interrupt 是「未收 [DONE]」时的显式中断
// 前置失败错误体 = ai::protocol::ProviderError（序列化 { kind, code, message }）

pub(crate) fn handle_guide(...) -> impl IntoResponse;      // SSE
pub(crate) fn handle_user_interrupt(...) -> impl IntoResponse;
```
