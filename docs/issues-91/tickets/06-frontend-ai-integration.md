# 06: 前端 AI 接入端到端（api / stateMachine / conversation / app 接线）

**What to build:** 接通「点分析 → 后端 SSE → 对话区双流渲染 + 历史 + 失败/中断」的完整链路：`ai/api.ts`（`AiApi.startGuide`/`interrupt_by_user` + `GuideEvent`/`ProviderError`/`GuideRequest`）、`stateMachine.ts`（`GuideMachine`/`GuidePhase`/`GuideState`）、`conversation.ts`（双流渲染）、`guideMode` 接线。端到端产出一条**真实 AI 流式建议**；`GuideMachine.start()` 一次 = 一次 `Guide::suggest`。


**Blocked by:** 04, 05

**Status:** ready-for-agent

- [ ] 点「分析」→ `AiApi.startGuide(sessionId, req, onEvent, onProviderError)` 消费 SSE → `GuideMachine` 状态推进 `idle→running→done/interrupted/preflight-failed`（`GuideState` 累积 `reasoning`/`content`）。
- [ ] 双流渲染：`reasoning_content`（浅色小字、整块可折叠）/ `content`（正常字体、不折叠）；`SUGGEST {"row":N,"col":M}` 与 `SUGGEST null` 只是文本——**不解析、不突出**。
- [ ] 历史绑定当前局；分析进行中历史不可点；「输入格式」变更 → 确认弹窗 + 清历史。
- [ ] 失败两型：前置失败（4xx/5xx）→ 按 bucket 文案 alert 弹窗（阻塞、确定）；流中断 → 保留已出字 + 末尾红字「已中断:<reason>」，**不弹窗**。
- [ ] 「中断」= 分析按钮的 running 态 → `interrupt_by_user(sessionId)`（前端保持 SSE 不 abort）。
- [ ] 每次 `start` 发**当前棋盘**、不缓存失败快照；AI 失败不影响棋盘上这局、无自动重试、无「重试」按钮。

### 接口契约

```ts
// ai/api.ts
export type BoardFormat = "simple-text" | "emoji" | "full-coordinates" | "image";
export type InterruptReason = "user_interrupt" | "rate_limit" | "timeout" | "upstream_error" | "unknown";
export type GuideEvent =
  | { kind: "reasoning"; text: string }
  | { kind: "content"; text: string }
  | { kind: "sse_done" }                          // 前端读到 SSE [DONE] 时合成；wire 不产 sse_done
  | { kind: "interrupt"; reason: InterruptReason };
export type ProviderError = { kind: "config" | "upstream"; code: number | null; message: string };   // = 后端 ai::protocol::ProviderError
export interface GuideRequest { format: BoardFormat; imageDataUrl?: string; }   // 不带 model
export interface AiApi {
  startGuide(sessionId: string, req: GuideRequest, onEvent: (e: GuideEvent) => void, onProviderError: (e: ProviderError) => void): void;
  interrupt_by_user(sessionId: string): Promise<unknown>;
}
// 前端不解析 SUGGEST（#95）；GuideEvent 无坐标字段；image base64 由 ai/screenshot.ts 收集进 GuideRequest.imageDataUrl

// ai/stateMachine.ts
export type GuidePhase = "idle" | "running" | "done" | "interrupted" | "preflight-failed";
export interface GuideState { phase: GuidePhase; reasoning: string; content: string; interruptReason?: InterruptReason; providerError?: ProviderError; }
export interface GuideMachine { start(req: GuideRequest): void; interrupt_by_user(): Promise<void>; reset(): void; onState(cb: (s: GuideState) => void): () => void; }
export function createGuideMachine(deps: { api: AiApi; newSessionId: () => string }): GuideMachine;
// 历史绑定一局 / 输入格式变更确认+清历史 的判定放组装层（app/），状态机只负责 phase + 累积文本

// ai/conversation.ts
export function createConversation(container: HTMLElement): { render(state: GuideState): void };
// reasoning_content→浅色小字可折叠；content→正常字体不折叠；SUGGEST/SUGGEST null 只是文本
```
