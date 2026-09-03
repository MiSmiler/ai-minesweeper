# 06: 前端 AI 接入端到端（api / stateMachine / conversation / app 接线）

**What to build:** 接通「点分析 → 后端 SSE → 对话区双流渲染 + 历史 + 失败/中断」的完整链路：`ai/api.ts`（`AiApi.startGuide`/`interrupt_by_user` + `GuideEvent`/`ProviderError`/`GuideRequest`）、`stateMachine.ts`（`GuideMachine`/`GuidePhase`/`GuideState`）、`conversation.ts`（双流渲染）、`guideMode` 接线。端到端产出一条**真实 AI 流式建议**；`GuideMachine.start()` 一次 = 一次 `Guide::suggest`。

**Coverage seams:** S7、S8、S9、S12

**Blocked by:** 04, 05

**Status:** ready-for-agent

- [ ] 点「分析」→ `AiApi.startGuide(sessionId, req, onEvent, onProviderError)` 消费 SSE → `GuideMachine` 状态推进 `idle→running→done/interrupted/preflight-failed`（`GuideState` 累积 `reasoning`/`content`）。
- [ ] 双流渲染：`reasoning_content`（浅色小字、整块可折叠）/ `content`（正常字体、不折叠）；`SUGGEST {"row":N,"col":M}` 与 `SUGGEST null` 只是文本——**不解析、不突出**。
- [ ] 历史绑定当前局；分析进行中历史不可点；「输入格式」变更 → 确认弹窗 + 清历史。
- [ ] 失败两型：前置失败（4xx/5xx）→ 按 bucket 文案 alert 弹窗（阻塞、确定）；流中断 → 保留已出字 + 末尾红字「已中断:<reason>」，**不弹窗**。
- [ ] 「中断」= 分析按钮的 running 态 → `interrupt_by_user(sessionId)`（前端保持 SSE 不 abort）。
- [ ] 每次 `start` 发**当前棋盘**、不缓存失败快照；AI 失败不影响棋盘上这局、无自动重试、无「重试」按钮。
