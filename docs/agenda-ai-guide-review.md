# AI 引导模式（#91）审阅 Agenda —— 逐个对齐待办

> 这是 issue **#91** 的 `docs/spec-ai-guide-mode.md` / `docs/seams.md` 审阅过程中，与用户**逐个对齐**的待办跟踪表。
> 流程：逐个确认 → 定案 → 落进 spec/seams 文档 → 提交。顺次讨论；新点追加到末尾。
> 状态：`- [x]` = 已定案（附提交哈希），`- [ ]` = 待对齐。

- [x] **`#96`「8 项」→ session 策略**（未确定项）：4 形式 × 2 session 策略。**定案 C**：只实现 `per-analysis`；UI 预留下拉、`per-game` 标注「(未实现)」置灰；切会话策略 = 弃局开新局。`903c4bc`
- [x] **`BoardFormat` serde rename**（wire 契约）：**定案** —— 锁 `kebab-case`（后端 `#[serde(rename_all = "kebab-case")]`，与前端 kebab 对齐）。
- [x] **base64 留底写盘无 seam**（spec→seams gap）：**定案** —— 留底在 `ai_adapter::Guide::suggest` 内做（内部副作用，不加 pub 接口）。
- [ ] **`PlayMode`(驼峰) vs `PlayModeName`(kebab)**（命名/形态）：类型形态 + UI label 区分。
- [ ] **`deepseek-v4-pro` 定位**（模型）：是否进本 map。
- [ ] **`--test-ai-chat` 进 seams 目录树**（可选）：补不补。
- [ ] **用户故事 8「我选的模型」措辞**（措辞）：与「前端不带 model」的内部矛盾。
- [ ] **`Guide::suggest` 非并发安全注记**（接受项）：补不补注解。
