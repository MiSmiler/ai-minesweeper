# 05: 前端 app/ 外壳（PlayMode + mode-switcher + guide 三区）

**What to build:** 前端组装层：`PlayModeName`/`mountMode`/`renderModeSwitcher`，组装出 `AiGuide` 组合——左上完整照搬的棋盘（独立 game client）、左下仪表盘（分析/中断、输入格式下拉、session 策略下拉、行列号 checkbox、历史）、右侧对话框空壳。顶栏 mode-switcher 切换 = 弃局开新局；`.top-bar` 更名 `.game-top-bar`。AI 分析先用 stub（本 ticket 不接后端）。**同时定义前端 wire 类型契约**（`AiApi`/`GuideEvent`/`GuideRequest`/`ProviderError`/`BoardFormat`，供 `AppDeps` 注入与 stub 分析按钮使用；真实消费 SSE 的实现留给 06）。**注：`PlayMode`（概念驼峰 `SinglePlay`/`AiGuide`）与 `PlayModeName`（kebab 运行时标识）不强行统一。**


**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 顶栏 mode-switcher 显示 SinglePlay / AiGuide，切换 = 弃局开新局（两模式互斥、不共享同一局）；switch 到 AiGuide 拿到全新初始棋盘。
- [ ] AiGuide 棋盘可照常 Reveal / Flag / Chord（独立 `createGameClient`，与 SinglePlay 互不干扰）；AI 任何操作/失败不影响棋盘操作。
- [ ] 仪表盘 UI 齐全：「分析 / 中断」同按钮双态、「输入格式」下拉（4 形式）、「session 策略」下拉（`per-analysis` 可用、`per-game` 标「(未实现)」置灰不可选）、「行列号」checkbox、历史列表（空）。
- [ ] 右侧对话框空壳存在；「分析」按钮先接 stub（占位/提示），不阻塞本 ticket；历史绑定当前局、输入格式变更→确认+清历史（判定放组装层）。
- [ ] `.top-bar` 更名 `.game-top-bar`；`AppDeps{getPlayMode, aiApi, captureBoardImage}` 注入点就绪（`captureBoardImage` 类型 = 截图函数签名；`createBoardAxis` 纯 DOM 直接 import）。
- [ ] 前端 wire 类型契约（`AiApi`/`GuideEvent`/`GuideRequest`/`ProviderError`/`BoardFormat`）已定义；`AppDeps.aiApi` 用 mock/stub 实现（供「分析」按钮占位 hook），不依赖后端。

### 接口契约

```ts
// ai/api.ts —— 前端 wire 类型契约（本 ticket 定义；实现消费 SSE 在 06）
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

// app/ —— 组合层（文件拆分源自 seams.md 首部目录树）
// app/main.ts    ：入口 —— 读初始 mode → mountMode + renderModeSwitcher + onSwitch（改造自现 main.ts）
// app/mode.ts    ：PlayModeName / AppDeps / mountMode（分发） + renderModeSwitcher（UI 控件）
// app/singleMode.ts：composeSingleMode —— 从现 main.ts 抽出 single 组合；供 mountMode("single")
// app/guideMode.ts ：composeGuideMode —— 新增，拼 ai/ slice

// app/mode.ts
export type PlayModeName = "single" | "ai-guide";
export type SessionStrategy = "per-analysis" | "per-game";   // per-game 未实现，UI 置灰 + 标注「(未实现)」
export type CaptureBoardImage = (boardEl: HTMLElement, opts?: { pixelRatio?: number }) => Promise<string>;   // = ai/screenshot.ts

export interface AppDeps {
  getPlayMode(): PlayModeName;
  aiApi: AiApi;
  captureBoardImage: CaptureBoardImage;   // 浏览器真实截图（html-to-image），jsdom 需 stub；createBoardAxis 纯 DOM 直接 import 不进 AppDeps
}
export function mountMode(mode: PlayModeName, root: HTMLElement, deps: AppDeps): () => void;
export function renderModeSwitcher(root: HTMLElement, current: PlayModeName, onSwitch: (m: PlayModeName) => void): void;

// app/singleMode.ts
export function composeSingleMode(root: HTMLElement, deps: AppDeps): { dispose(): void };   // 现有 single 组合（从现 main.ts 抽出）

// app/guideMode.ts
export function composeGuideMode(root: HTMLElement, deps: AppDeps): { dispose(): void };   // 新增，拼 ai/ slice

// PlayMode（概念驼峰 SinglePlay/AiGuide）= 前端组装层概念，无后端类型；PlayModeName（kebab）= 运行时标识；UI 显示名用驼峰 label
```
