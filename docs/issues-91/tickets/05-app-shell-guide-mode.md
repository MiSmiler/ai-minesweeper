# 05: 前端 app/ 外壳（PlayMode + mode-switcher + guide 三区）

**What to build:** 前端组装层：`PlayModeName`/`mountMode`/`renderModeSwitcher`，组装出 `AiGuide` 组合——左上完整照搬的棋盘（独立 game client）、左下仪表盘（分析/中断、输入格式下拉、session 策略下拉、行列号 checkbox、历史）、右侧对话框空壳。顶栏 mode-switcher 切换 = 弃局开新局；`.top-bar` 更名 `.game-top-bar`。AI 分析先用 stub（本 ticket 不接后端）。**注：`PlayMode`（概念驼峰 `SinglePlay`/`AiGuide`）与 `PlayModeName`（kebab 运行时标识）不强行统一。**

**Coverage seams:** S12（+ S11 的 `captureBoardImage` 类型、S1 复用）

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 顶栏 mode-switcher 显示 SinglePlay / AiGuide，切换 = 弃局开新局（两模式互斥、不共享同一局）；switch 到 AiGuide 拿到全新初始棋盘。
- [ ] AiGuide 棋盘可照常 Reveal / Flag / Chord（独立 `createGameClient`，与 SinglePlay 互不干扰）；AI 任何操作/失败不影响棋盘操作。
- [ ] 仪表盘 UI 齐全：「分析 / 中断」同按钮双态、「输入格式」下拉（4 形式）、「session 策略」下拉（`per-analysis` 可用、`per-game` 标「(未实现)」置灰不可选）、「行列号」checkbox、历史列表（空）。
- [ ] 右侧对话框空壳存在；「分析」按钮先接 stub（占位/提示），不阻塞本 ticket；历史绑定当前局、输入格式变更→确认+清历史（判定放组装层）。
- [ ] `.top-bar` 更名 `.game-top-bar`；`AppDeps{getPlayMode, aiApi, captureBoardImage}` 注入点就绪（`captureBoardImage` 类型 = 截图函数签名；`createBoardAxis` 纯 DOM 直接 import）。
