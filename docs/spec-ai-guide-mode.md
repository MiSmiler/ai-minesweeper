# Spec: AI 看我玩（`AiGuide`）引导模式 — 实施规格

> 本文件是由 issue **#91**（map）及其 9 个已闭合子 ticket（#92 #93 #94 #95 #96 #97 #104 #111）
> 综合而成的**实施规格草稿**。它把各子 ticket 的结论固化为一个可由 agent 执行的 spec。
> **状态：草稿**，待人工审阅后决定是否发布到 issue tracker（发布时贴 `ready-for-agent` 标签）。
> 草稿阶段**不以任何形式写入 GitHub**。

- 来源子 ticket：`#92` DeepSeek API、`#93` 截图/图像路径、`#94` 4 种呈现形式/prompt、
  `#95` AI 输出契约、`#96` PlayMode 前端架构、`#97` 失败/报错、`#104` 后端 AI 代码组织、`#111` 行列号。
- 代码状态参考：当前在 `wip/issues-91` 分支，`src/` 只有 `core/` + `server/`，前端只有 `game/` slice，
  **尚无任何 AI 实现**（无 `ai/`、无 `/ai/...` 路由、无 `PlayMode`）。本 spec 描述的是**将要实现**的目标。

---

## Problem Statement

现在的扫雷只有「**我玩**」（`SinglePlay`）一个人默默玩、默默死。玩家想让一个强大的 AI 看着自己玩：
把自己**当前这一局的棋盘**给 AI（DeepSeek），AI「指点江山」——先讲讲它怎么推理的，再告诉我「下一步点哪一格」。
人照着点，继续玩下去。这是一种「AI 当陪练/军师」的体验，目的地是**验证 AI 能否看懂扫雷局面**，而不是让 AI 自己把棋玩完。

痛点与约束：

- 给 AI 的输入**只能包含玩家可见信息**（哪个格 hidden / flagged / revealed 的数字），
  **绝不能泄露真正的 Mine 布局**——否则就是作弊，失去「验证 AI 理解力」的意义。
- 推理要「流式」呈现（DeepSeek 的思考链 `reasoning_content` 与答案 `content` 分开流），
  人边看边体会它在想什么；但**前端不做解析、不做高亮**，坐标是给人读文本定位、自己点的。
- 有个细微但关键的事实要处理：玩到一半，玩家可能 over-flag，`flags_remaining ≠ mine_count`，
  所以发给 AI 的头部必须**同时带 `Mine count` 和 `Flags remaining`**，否则 AI 会被误导。
- 失败/报错不能毁掉这一局：AI 分析失败只收束「分析」这件事本身，**绝不影响棋盘场上的 Game**。
- 前后端始终**只有一局在跑**（单 `Game` 模型），多局实例不属于本 map。

---

## Solution

给扫雷接入一个 `PlayMode` = `AiGuide`（「AI 看我玩」，即 guide 模式）。

- 玩家照常在这个模式的棋盘上玩（完全复刻单机操作），游戏区是 `SinglePlay` 的**完整照搬**
  （不是同一块 `.board` 换皮，而是本模式自己的 DOM + 自己的 game client）。
- 左下仪表盘：**分析**按钮、「输入格式」下拉（4 种呈现形式）、**行列号**辅助标记 checkbox、**历史**列表。
- 右侧对话框：`reasoning_content`（思考链，浅色小字、整块可折叠）+ `content`（可读推理 + 末尾
  `SUGGEST {"row":N,"col":M}` 行，正常字体、不折叠）。坐标 0-based，人读文本、靠 `#111` 行列号定位格子。
- 点「分析」：把当前局**玩家可见状态**（`snapshot` 语义）按选中的形式序列化，Rust 后端代理转发给
  DeepSeek（OpenAI 兼容 `POST /chat/completions`），SSE 流式返回推理 + 建议。
- 顶栏 **mode-switcher**：`SinglePlay` ↔ `AiGuide` 互斥；**切换即弃局开新局**，无「换个模式接着同一局」。
- 保留原「我玩」（`SinglePlay`）模式不变。

失败处理（贴合 #97，**无「降级」分析路径**，只有「失败处理+报错」）：

- **前置失败**（HTTP 4xx/5xx、未流内容）→ 浏览器 alert 弹窗（阻塞、确定），文案分桶：
  配置错误 `400/401/402` | 上游瞬时 `429/500/503/网络`。
- **流中断**（已流、未收到 `[DONE]`）→ 保留已出字 + 末尾红字「已中断:<reason>」，**不弹窗**。
- 终止状态统一由后端 SSE 的**终止 event**裁决（reason kind：
  `user_interrupt` / `rate_limit` / `timeout` / `upstream_error` / `unknown`）。
- **用户中断通道**：前端保持 SSE 不 abort，发 `POST /ai/guide/<id>/interrupt`，后端用
  `CancellationToken` cancel 上游生成，并在**同一 SSE** 发 `{reason:"user_interrupt"}` 终止 event。
- 失败**不影响场上的 Game**，不自动重试、无「重试」按钮；再点「分析」= 开一轮新分析（发当前棋盘，
  不缓存失败快照）。

---

## User Stories

按 actor 分，主体是**扫雷玩家**（`the player`），辅以**系统/开发者**视角。编号连续。

### 进入与模式

1. 作为玩家，我想在顶栏的 mode-switcher 里看到 `SinglePlay` 和 `AiGuide` 两个模式，并随时切换，这样我可以自由进出「AI 看我玩」。
2. 作为玩家，我想在切到 `AiGuide` 时拿到一个全新（跟刚从 `SinglePlay` 切过来时一样的初始）棋盘，这样 AI 的「看你玩」是从一盘新局开始。
3. 作为玩家，我想在从 `AiGuide` 切回 `SinglePlay` 时开一盘新局（而不是把 guide 那局带回去），这样两个模式不会共享同一局、不会串状态。
4. 作为玩家，我想在切换模式时**明确知道当前局会被放弃**（切走即弃、切回重置），这样我不会误以为可以「换个模式接着玩同一局」。

### 在 guide 棋盘上玩

5. 作为玩家，我想在 `AiGuide` 的游戏区里**照常**用鼠标 Reveal / Flag / Chord（和 `SinglePlay` 完全一样），这样 AI 看的是我真实的操作。
6. 作为玩家，我想让这个游戏区有自己独立的 game client（不是复用 `SinglePlay` 的同一个实例），这样两个模式互不干扰。
7. 作为玩家，我不想让 AI 的任何一个操作/失败影响我棋盘上的操作，这样「AI 看我玩」永远只是陪练，不夺走我的键盘。

### 点「分析」、看回复

8. 作为玩家，我想点「**分析**」把当前局发给我选的 AI 模型，这样它能针对这盘真实的局面给建议。
9. 作为玩家，我想看到**流式**返回的回复，这样我能边看它推理边等结论，而不是干等一大段一起出来。
10. 作为玩家，我想把**思考链**（`reasoning_content`）用**浅色小字、整块可折叠**显示，这样我可以先看结论、想细看再展开。
11. 作为玩家，我想把**答案正文**（`content`）用**正常字体、不折叠**显示，这样它是我默认阅读的主体。
12. 作为玩家，我想看到回答**末尾**有一行 `SUGGEST {"row":N,"col":M}` 的坐标，这样我能快速定位建议格。
13. 作为玩家，我想**不**让前端去解析或高亮这个坐标，这样它只是一段普通文本，我可以像读一句话一样读它。
14. 作为玩家，当 AI 无法给出任何建议时，我想看到 `SUGGEST null`（同样只是文本、显示「无法给出建议」），这样我知道它真的没辙而不是漏了返回。

### 坐标定位（行列号）

15. 作为玩家，我想在 guide 棋盘**外圈**看到 0-based 的行列号轴标（`boardAxis`/`axisLabel`），这样我能把 `SUGGEST {"row":N,"col":M}` 对到格子。
16. 作为玩家，我想用 checkbox「**行列号**」开关这个轴标，且默认**关**、切走模式即丢、切回重置、不持久化，这样我需要时才开。
17. 作为玩家，我想让这个轴标**纯视觉**——不动当前局、不清历史、不发不同输入、不弹窗，这样它只帮我读坐标、不改变任何玩法。
18. 作为玩家，我想让轴标是 `.board` 外的**绝对定位 overlay**、`pointer-events:none`，这样它不挡点击、也**不进入 `.board` 截图**。
19. 作为玩家，我想让轴标对**4 种 AI 输入形式零影响**（正交），这样我开不开它不影响发送给 AI 的内容。

### 输入形式

20. 作为玩家，我想在「输入格式」下拉里选 AI 看到的棋盘形式，这样我可以检验 AI 在不同输入形态下的理解力。
21. 作为玩家，我想看到**4 种形式**：A 简单字符、B emoji、C 完整坐标、D 图像，这样本 map 的目的（验证 AI 能否看懂）被完整覆盖。
22. 作为玩家，我想让**每一种形式都带头部**（`Difficulty` / `Rows` / `Cols` / `Mine count` / `Flags remaining` / `Game state`），这样 AI 有正确的数值上下文。
23. 作为玩家，我想特别让头部**必带 `Mine count`**（因为玩到一半 `flags_remaining ≠ mine_count`，只给剩余旗数会误导 AI），这样它能正确推理。
24. 作为玩家，我想让 4 种形式**共用同一个 system prompt**（坐标 0-based、只看玩家可见信息、末尾 `SUGGEST` 契约），这样行为跨形式一致。

### 图像形式（#93、#92）

25. 作为玩家，我想在「D 图像」形式下用 `html-to-image` 对 `.board` 截一张 PNG，这样 AI 能看到「图的」棋盘。
26. 作为玩家，我想让截图走前端，`pixelRatio` 接口预留、默认不放大，这样图不至于过大。
27. 作为系统，我想让后端在转发 DeepSeek 前把收到的 base64 图**留底写盘**（`<exe_dir>/base64_img/YYYYMMDD_<seed>_<seq>.png`，不阻断发送），这样便于排查视觉理解问题。
28. 作为玩家，我想让图像形式走 vision-exp，用 `image_url`（`content` 数组、每图 ≤384 token），这样 AI 能真的「看图」。
29. 作为开发者，我想让 Playwright 仅用于开发/工具的截图，**不作运行时 capture**，这样运行路径干净、可测。

### 历史与输入格式变更（#96）

30. 作为玩家，我想让「历史」**绑定到当前这一局**（每局清空/独立），这样历史不会串到另一盘。
31. 作为玩家，我想在**分析进行中**禁点历史里的条目，这样我正在看的一次分析不会被替换。
32. 作为玩家，我想在更改「输入格式」时**清空历史**，并弹一个**确认弹窗**，这样我不会误以为旧格式的分析仍适用。
33. 作为玩家，我想在点「**中断**」时能停止当前分析，这样我不用干等它慢慢推理。
34. 作为玩家，我想让「分析 ↔ 中断」是同一个按钮的两种状态，这样 UI 语义清楚。

### 失败与报错（#97）

35. 作为玩家，当 AI 请求**前置失败**（HTTP 400/401/402/422/429/500/503 或网络错误、未流内容）时，我想看到**分桶文案的 alert 弹窗**（阻塞、确定），这样我知道是配置错了还是上游临时抽风。
36. 作为玩家，当**流中断**（已流、未收到 `[DONE]`）时，我想看到**已出内容保留 + 末尾红字「已中断:<reason>」**，且**不弹窗**，这样我至少能看到它已经想到哪一步。
37. 作为玩家，我想让 AI 的**任何失败都不影响我棋盘上的这局**，这样分析挂了，我的棋还能继续下。
38. 作为玩家，我想让失败**不自动重试、也没有「重试」按钮**，这样我不会反复被同一份失败快照折腾。
39. 作为玩家，我想在失败后再点「分析」能**开一轮新分析**（发当前棋盘、不缓存失败快照），这样我能对最新的局面再问一次。
40. 作为玩家，我想让**用户中断**走 `POST /ai/guide/<id>/interrupt`（前端保持 SSE 不 abort），这样能干净地停掉上游生成。

### 后端与保密（#92、#104）

41. 作为系统，我想让 `/ai/...` 路由由 `server` 持有（薄传输层），`ai_adapter` 不依赖 `server`，这样职责清晰（ADR-0013）。
42. 作为系统，我想让 AI 组织为 `ai`（通用 agent runtime、与 `core` 解耦）+ `ai_adapter`（扫雷绑定、依赖 `core`+`ai`），这样 `ai` 可复用、可未来抽 crate（ADR-0013）。
43. 作为系统，我想让 `ai` 内分 `agent → provider`，`provider` 只有 `Provider` seam + `deepseek` 实现，这样未来可加别的 provider。
44. 作为系统，我想做**一次性顾问** `ai_adapter::suggest()`（single-turn、只读），而不是让 AI 自主点格子，这样本 map 的「建议 + 人照做」与 `#24` 的「AI 自主玩」区分开。
45. 作为系统，我想给 AI 的输入**只含玩家可见信息**（`snapshot` 语义：hidden/flagged/revealed+数字），**绝不泄露 Mine 布局**，这样「验证 AI 理解力」才有意义。
46. 作为系统，我想让给 AI 的 payload **不含 mine 位置/总量等机密**（只带 `Mine count` 这个 recipe 常量），这样前端薄客户端不至于背着雷布局。
47. 作为系统，我想让 `ai_adapter` 的工具绑定走 `GameHandle`（而非硬编码单一 `Game`），这样为未来「AI 和我玩」双 Game 留口（ADR-0013）。

### 模型与流（#92、#95）

48. 作为系统，我想让 DeepSeek 走 OpenAI 兼容 `POST /chat/completions`（`base_url=https://api.deepseek.com`、Bearer 认证），这样集成成本低。
49. 作为系统，我想支持 `deepseek-v4-flash` / `deepseek-v4-pro` / `deepseek-v4-flash-vision-exp`，这样能按需选快/强/视觉模型。
50. 作为系统，我想让 SSE 流把 `content` 和 `reasoning_content` **分开**，这样前端能双流呈现（#95）。
51. 作为系统，我想遵守「带 `tools` 就必须回传 CoT」的约束，这样未来进 `AiPlay` 时思考链不被丢弃。
52. 作为系统，我想处理 DeepSeek 的错误码 `400/401/402/422/429/500/503`，并把它们映射成 #97 的分桶（前置失败对应 400/401/402 配置错误，429/500/503/网络 对应上游瞬时）。

---

## Implementation Decisions

> 术语按 `CONTEXT.md` 词汇表：`Game`、`Cell`、`Position`、`CellState`、`GameState`、`Flag Budget`、
> `Flags Remaining`、`Feature`、`PlayMode` 等。不做 `Feature`→领域重构（#98，独立 chore，解耦）。
> 架构内核见 **ADR-0012**（PlayMode 前端组合）与 **ADR-0013**（AI 子系统）。

### 1. PlayMode（前端组装层）— ADR-0012

- 概念定型为 **`PlayMode`**（open enum）：`SinglePlay` / `AiGuide`（未来 `AiPlay` / `AiPlayWithMe`）。
  它属**前端组装层**，与核心层的 `Feature`/`Features`（影响玩法的 opt-in）**解耦**。
- 一个 `Game` 在同一时刻只有**一个** `PlayMode`。两模式**互斥**；切换即放弃当前局并开新局
  （后端仍是单 `Game`）；没有「换个模式接着同一局」。
- 每个 `PlayMode` 是 `app/` 里一个独立组合，各自套用一个独立 game slice（`createGameClient`），
  guide 还套用一个 `ai/` slice；切换 = 拆掉重建该组合。
- 顶栏一个 **mode-switcher bar**。`.top-bar` 更名 `.game-top-bar`（避免与 mode-switcher 混淆的命名，属组装层）。
- `AiGuide` 组合分**三区**：
  - **左上游戏区**：`SinglePlay` 的完整照搬（独立 DOM + 独立 game client）。
  - **左下仪表盘**：「分析」按钮（含「中断」）、「输入格式」下拉、**行列号** checkbox、**历史**列表。
  - **右侧对话**：思考区（`reasoning_content`）+ 正文区（`content`）。
- **历史**：一局绑定、独立；「输入格式」任何变更 = 清空历史 + 确认弹窗；分析进行中历史不可点。
- 本 map **只做前端外壳 + stub/seam** 到未来的 `ai/` slice（`#96` 只交外壳；`ai/` slice 的实现在本 spec 一并规划，见下）。

### 2. AI 子系统组织 — ADR-0013

- **目录 mod，不拆 crate**。`ai`（通用 agent runtime，与 `core` 解耦、不知道扫雷）+ `ai_adapter`
  （扫雷绑定，依赖 `core` + `ai`）与 `core`/`server` 同构。`server` 持 `/ai/...` 薄路由；
  `ai_adapter` **不依赖** `server`；`main` 组合一切（建 `Game`、建 `ai` agent、用 `ai_adapter` 注册工具、接路由）。
- `ai` 内分 `agent`（引擎：`Agent`、`Tool`、`Session`、`run_loop`）→ `provider`（`Provider` seam + `deepseek` 实现）。
  `agent → provider` 是唯一内部依赖，`provider` 不依赖任何内部模块。
  **`Provider` seam 存在是因为未来不止 DeepSeek 一个 provider**。
- 一次性顾问 = **`ai_adapter::Guide::suggest()`**：single-turn、只读，注入棋盘、返回一份分析 + 一个建议。
  这里 `Guide` 是**正名**（模式 `AiGuide` 里的 AI 角色）；`assist` 只是 issue 讨论里的工作术语，非代码/文档符号。
- 未来持久循环模式 = `ai_play`（跑工具循环、读写 `Game`），名称暂定，本 map **不实现**。
- 工具绑定是参数而非写死单一游戏：`ai_adapter::tools` 从传入的 `GameHandle` 构建 `ai::Tool`s。
  今天各模式共享后端单 `Game`（`Arc<Mutex<Game>>`）、切模式触发新局；未来 `AiPlayWithMe` 需要两个
  `Game` 实例——把工具绑到 handle 可让 adapater 不变。本 map 只实现单次 `suggest`（无工具循环）。

### 3. 给 AI 的输入只含玩家可见信息（保密）

- 输入**只含 `snapshot()` 语义**（每个 `Cell` 的 `CellState` + revealed 的数字），**绝不泄露 Mine 布局**。
  `ai_adapter` 序列化时只读 `core::Game` 的**可见**侧（`cell_view`），不读 `mines`。
- 头部 `Mine count` 是 recipe 常量（`difficulty.mine_count()`），可以给；但**绝不给**实际 mine 位置/布局。
- 这是本 map 的**硬约束**：AI 看不到真雷，只能靠已揭数字 + `Mine count` 推理。

### 4. 4 种呈现形式 + 共享 system prompt（#94）

- 4 形式共用一套**头部**（必带 `Mine count`，因 mid-game `flags_remaining ≠ mine_count`）：
  `Difficulty` / `Rows` / `Cols` / `Mine count` / `Flags remaining` / `Game state`。
- **不加行号前缀**（用 prompt 里的几何说明替代）：坐标 **0-based**，术语用「0-based」。
- 输出契约：`role:"system"`（共享契约）+ 各形式 `role:"user"`（该形式的棋盘 payload）。
- 4 形式：
  - **A 简单字符**：`.`=hidden、`*`=revealed mine（仅 Lost）、`F`=flag、`0-8`=revealed 数字。
  - **B emoji**：`⬛`=hidden、`💣`=revealed mine（仅 Lost）、`🚩`=flag、`⬜`=revealed 无雷(0)、`1️⃣`-`8️⃣`=revealed 有雷；
    prompt 强调「每个 emoji=一格」。emoji 多码点**只影响**「反查网格的程序」，demo 不做反查 → 不构成障碍。
  - **C 完整坐标**：每格写成 `[row][col]:x`，每格自报坐标，无需数行列。
  - **D 图像**：`html-to-image` 对 `.board` 截图（PNG base64）→ 后端 → vision-exp `image_url`。
    图像独立验证视觉理解，**不设文字退化兜底**。
- 真实 DeepSeek 验证留实现期。推荐例见 `docs/board-format-prototype.md`（含自洽棋盘样例与目标 `SUGGEST` 输出）。

### 5. AI 输出契约（#95）

- guide 的交付物 = **可读分析 + 坐标**，目的在验证 AI 对棋盘的分析能力。
- **不做前端解析/高亮**：`SUGGEST {"row":N,"col":M}` 机器标记行**保留但无人消费**，人直接读文本
  （含末尾 `SUGGEST` 行）定位格子。
- 建议**单个坐标**；未来可扩展多个。坐标 **0-based**；「人照做」定位以 **#111**（行列标号）为前提。
- 对话框**双流展示**：`reasoning_content` 浅色小字、整块可折叠；`content` 正常字体、不折叠
  （仿 DeepSeek 网页版）。`SUGGEST null` 也只是文本，显示「无法给出建议」。

### 6. 行列号辅助标记（#111）

- `AiGuide` 棋盘**外圈** 0-based row/col 轴标（`boardAxis`/`axisLabel`），`.board` 外**绝对定位
  overlay**、`pointer-events:none`、**不进 `.board` 截图**、对 4 种 AI 输入形式**零影响**（正交）。
- **不预留 gutter**（负偏移占天然留白，被裁再补 padding）。
- checkbox「行列号」随仪表盘落位；**纯视觉**——不动当前局、不清历史、不发不同输入、不弹窗。
- **默认关**、guide 组件内状态（切走即丢、切回重置、不持久化）。
- 术语**不入** `CONTEXT.md`、**不立 ADR**（纯前端小组件）。

### 7. DeepSeek API（#92）

- 模型：`deepseek-v4-flash` / `deepseek-v4-pro` / `deepseek-v4-flash-vision-exp`
  （1M 上下文 / 384K max output，默认 thinking）。
- OpenAI 兼容 `POST /chat/completions`（`base_url=https://api.deepseek.com`，Bearer 认证）。
- SSE 流式：`content` + `reasoning_content` **分离**（带 `tools` 必须回传 CoT）。
- vision-exp 传图走 `content` 数组 `image_url`（每图 ≤384 token）。
- tool use 支持（函数参数 JSON 需自校验）。
- 错误码：`400/401/402/422/429/500/503`。

### 8. 截图 / 图像路径（#93）

- 图像在**前端**用 `html-to-image` 截 `.board`，`pixelRatio` 接口预留、默认不放大，PNG base64 → 后端。
- 后端转发 DeepSeek 前先**留底**写盘 `<exe_dir>/base64_img/YYYYMMDD_<seed>_<seq>.png`（**不阻断发送**），
  再以 `image_url` 传给 vision-exp。
- Playwright 仅作开发/工具时截图，不作 runtime capture。
- 表头字段见 #94（头部带 `Mine count` 等）。

### 9. 后端 `/ai/...` 路由与失败/终止（#97）

- 路由由 `server` 持有（薄传输层），调 `ai_adapter::suggest()`。
- 失败**不影响对局**（core `Game` 独立），只收束分析状态机。**无自动重试、无「重试」按钮**；
  再点「分析」开一轮新分析（发当前棋盘、不缓存失败快照）。
- 呈现分两型：
  - **①前置失败**（HTTP 4xx/5xx、未流内容）= 浏览器 alert 弹窗（阻塞、确定）。
    分桶文案：配置错误 `400/401/402` | 上游瞬时 `429/500/503/网络`。走 HTTP 状态码 + 结构化错误体。
  - **②流中断**（已流、未收 `[DONE]`）= 保留已出字 + 末尾红字「已中断:<reason>」，**不弹窗**。
- **终止状态统一由后端发 SSE 终止 event 裁决**，reason kind：
  `user_interrupt` / `rate_limit` / `timeout` / `upstream_error` / `unknown`。
- **用户中断通道**：前端保持 SSE **不 abort**，发 `POST /ai/guide/<id>/interrupt`；后端 cancel 上游生成
  （`CancellationToken`，生成侧 `select!`）并在同一 SSE 发 `{reason:"user_interrupt"}` 终止 event，
  前端据 event 渲染红字。`rate_limit` / `timeout` / `upstream_error` 同理。

### 10. API 契约 / wire 概览

- `POST /ai/guide/:id`：开始一轮分析，返回 SSE 流（`content` + `reasoning_content` + 终止 event）。
- `POST /ai/guide/:id/interrupt`：取消上游生成，驱动同一 SSE 的 `{reason:"user_interrupt"}` 终止 event。
- 请求体：所选 **presentation form** + **model**（+ 图像形式附带 `image` base64）。
  **文本形式（A/B/C）的棋盘由后端读自己的 `Game` 渲染**（单 `Game` 权威），前端**不**回传棋盘数据；
  图像形式（D）由前端 `html-to-image` 截 `.board` 并回传 `image`。请与 `docs/seams.md` S4 核对。
- 响应流在 `[DONE]` 处收尾；未收 `[DONE]` 即断 = 流中断（#97 ②）。

### 11. 前端 `ai/` slice（#96 补全）

- `frontend/src/ai/` slice（ADR-0011 已预留）：AI 对话、分析状态机、双流渲染、行列号组件、`html-to-image` 截图。
- `app/` 组装：mode-switcher + 各 `PlayMode` 组合；guide 组合拿 `ai/` slice + `game/` slice 拼。

---

## Testing Decisions

> 该 repo 的单测/行为测惯例（prior art）：
> 后端在**模块 seam** 处测外部行为、用 mock 替换依赖（`core/tests.rs` 只测 `Game`、`server/mod.rs` 测
> `apply_action` / `GameSnapshot::from_game`、`wire.rs` 测序列化；**不**起真实 HTTP 集成）。
> 前端在 `createGameClient(deps)` 的 **deps 注入 seam** 测组合（`client.test.ts` mock `post`/`fetchSnapshot`），
> 纯逻辑（gesture/preview/hitTest/snapshotRender）各自独立测。
> 本 spec 遵循同一精神：**只测外部可观察行为，不测内部实现细节**，用 mock 替换外部依赖。

**Seam 清单与各 seam 的 pub 接口形状（S1–S11）见 [`docs/seams.md`](seams.md)**——本小节只概览每侧的主 seam，具体类型/签名以及每处 `待确认` 分叉点都以该文件为准。

**拟用 seams（完整清单见 `docs/seams.md`，此处为概览）**：

1. **后端主 seam：`ai::provider::Provider`**（拟建）。
   这是 AI「大脑」的单点。`ai_adapter::suggest(game, form, provider)` 是顾问入口，测它时注入一个
   **mock `Provider`**，一次性覆盖：棋盘→prompt→provider→SSE 解析→`SUGGEST` 提取→返回分析。
   也经此 seam 测失败/终止路径（mock provider 返回错误码 / 触发 `[DONE]` 缺失）。

   - 说明：`core::Game` 是核心层唯一 seam；类比上，AI 运行时的单一 seam 就是其**可插拔 provider**。
     `suggest` 是用户「点问 AI」直接命中的入口，把整个往返（除真实 DeepSeek HTTP 之外）收在一个行为测里。

2. **`ai_adapter` 序列化/prompt 纯函数 seam**（拟建，次级）。
   `ai_adapter` 把 `Game` 序列化成 4 种形式 + 拼 system prompt。行为测覆盖不了「精确锁定契约」，
   故对 4 个形式输出与 system prompt 做**直接纯函数测**：断言头部字段齐全、坐标 0-based、不出现
   mine 布局信息、`SUGGEST` 契约行存在。这是 `seam 1` 的子 seam，仍指向外部可观察的结果（payload 内容）。

3. **`deepseek` provider 实现 seam**（拟建，次级）。
   该实现承载 HTTP/SSE 流式、`tool_use`、`image_url`/vision 细节。测它时需要**假 HTTP 层**（mock
   传输），验证 `content`/`reasoning_content` 分开、错误码映射、`[DONE]` 判定。

4. **`server` `/ai/...` 传输 seam**（拟建）。
   测 SSE 终止 event 状态机（#97）：前置失败→结构化错误体；流中断→终止 event（`user_interrupt`/
   `rate_limit`/`timeout`/`upstream_error`）。与 `server/mod.rs` 现在测 `apply_action` 同思路——在路由
   处理逻辑 seam 处测，不追求真实 HTTP 集成。

5. **前端组合 seam：`app/` 中 guide 组合的 deps 注入**（拟建，类比 `createGameClient`）。
   给 guide 组合注入 **mock `ai` api adapter** + **mock `game` client**，驱动可观察 UI 行为：
   mode 切换弃局开新局、双流渲染、输入格式变更→确认+清历史、中断→红字、行列号开关。

**哪些模块会被测**：`ai`（agent/run_loop/session + provider seam）、`ai_adapter`（序列化/prompt/`suggest`）、
`server`（`/ai/...` 处理）、前端 `ai/` slice 组件、`app/` 的 guide 组合。

**什么是一个好测试**：只断言**外部可观察行为**（给定一个 `Game` 的玩家可见状态 → 得到的 prompt/payload
正确、`SUGGEST` 契约正确；给定 mock provider → 分析、`[DONE]`、终止 event 正确；给定前端 mock deps →
能观察到的 UI/状态正确），**不**断言内部数据结构/private 方法/调用次数。保密性单独成测：
**任何发给 provider 的 payload 都不含 mine 布局**。

**关于「单 seam 的理想」**：本 feature 横跨 Rust 后端与 TS 前端两个 stack，repo 现有惯例已是
「每侧一个 seam」（后端 `core`、前端 `createGameClient`）。故本 spec 无法收敛为**字面上一个** seam；
最接近的是：后端以 `Provider` seam 为主、前端以 `app/` 组合 deps 注入为主，其余为次级纯函数测。

---

## Out of Scope

- **#24**：一套扫雷专用 harness，让 DeepSeek 通过函数调用**自主**玩到终局——与本 map 的「建议 + 人照做」不同目的地。
- **棋盘上高亮建议格**：前端**不**解析坐标、不做视觉高亮（#95）。`Preview` 一词在 `CONTEXT.md` 仍专指 Gesture 的 transient highlight，与 AI 建议无关。
- **Prank→Feature 领域重构**：独立 chore（#98），不阻塞本 map（双方解耦）。
- **多后端 `Game` 实例**：单局模型不变。
- **AI 分析的呈现丰富度**（安全区/危险区分级、多步推演、概率走势）：目的地是「单步建议 + 人照做」，不做分级/多步/概率。
- **AI 建议的置信度表达**（标注「这格是猜的」）：不做风险标注。
- **「我玩」复用分析层**：`SinglePlay` 不接入分析/呈现层，两边不合并。
- **`AiPlay` / `AiPlayWithMe` / `ai_play` 持久循环**：本 map 只实现一次性 `suggest`（无工具循环）。
- **`ai` 抽成独立 crate**：AI 边界仍在演进，且 `ai` 与 `core` 解耦，未来抽 crate 是机械动作，现在不抽。
- **多 provider**：只做 DeepSeek；`Provider` seam 为未来留口，但不实现其他 provider。
- **`tools`/`tool_use` 在顾问模式的实际调用**：顾问是 single-turn、只读、无工具；`tool_use` 支持仅为未来 `AiPlay` 准备。

---

## Further Notes

- **`输入格式(8项)` vs 原型 4 形式（#96 与 #94/#93 的潜在冲突）**：
  `docs/board-format-prototype.md` 与 #94 明确为 **4 种形式**（A/B/C/D）；但 #96 决策文本写「输入格式(8项)」。
  本 spec 以 **4 形式（#94 权威）** 为准，并把「8 项」视为**待澄清**——可能是 4 形式 × 某 2 态开关，
  或只是笔误。**请在审阅时定夺**：若确为 8 项，需给出 8 个具体项（如 4 形式 × 思考链开关？）。
- **坐标呈现形式边界**：`SUGGEST {"row":N,"col":M}` 的 `row`/`col` 均为 **0-based**；`SUGGEST null`
  表示无建议。该机器行**保留但无人消费**；建议**单个坐标**（未来可扩展多个）。
- **`flags_remaining` 可能为负**（over-flag），头部 `${flags_remaining}` 可能为负数——prompt 已说明，
  序列化时原样输出即可，不必 clamp。
- **真实 DeepSeek 验证留实现期**：本 spec 的样例/契约来自手造目标输出（#94 原型「样例输出」），
  非真实模型返回。实现后需用真实 vision/文本模型各跑一次验证。
- **命名**：模式名词用 `Guide`（AI 角色），`PlayMode` 变体用 `AiGuide`，接口用 `Guide::suggest()`；
  模块用 `ai` / `ai_adapter`（`assist` 只是 issue 工作术语，已弃用）。也不要新增 `Preview` 之外的高亮术语（#95 明确不解析、不高亮）。
- **图片留底**：`base64_img` 写盘用 `YYYYMMDD_<seed>_<seq>.png`；`<exe_dir>` 是进程可执行目录，
  注意可写性（失败不阻断发送）。
- **线程/并发**：后端单 `Game` 由 `Arc<Mutex<Game>>` 共享；`/ai/...` 只读棋盘（建议模式不写 `Game`），
  用 `suggest()` 时快照棋盘可见状态即可，避免长持锁阻塞玩家操作。
- **依赖**：后端需新增 HTTP client（如 `reqwest`）以发起 DeepSeek 请求；前端需新增 `html-to-image`
  （`#93`），均需在 `Cargo.toml` / `package.json` 中声明。
