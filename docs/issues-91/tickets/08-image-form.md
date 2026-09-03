# 08: 图像形式（ai/screenshot + D 形式流程 + 后端留底）

**What to build:** 图像形式端到端：前端 `captureBoardImage`（html-to-image）截棋盘 → 放进 `GuideRequest.imageDataUrl` → 后端按格式 D 切 vision-exp 分析 + **留底写盘**。同时固定「文本形式（A/B/C）由后端读自己 Game、前端只发 format（不带棋盘、不带 model）」的边界。


**Blocked by:** 03, 05, 06

**Status:** ready-for-agent

- [ ] `captureBoardImage(boardEl, {pixelRatio?})` 返回 PNG data URL（默认 `pixelRatio` 不放大）；经 `AppDeps.captureBoardImage` 注入（jsdom 无法真截图 → stub 可测），`createBoardAxis` 不经 AppDeps。
- [ ] 选「D 图像」→ 前端截图 → 后端 `suggest` 切 `VISION_MODEL`（vision-exp），`image_url` 每图 ≤384 token。
- [ ] 后端把 `req.image_data_url` 留底写盘 `<exe_dir>/base64_img/YYYYMMDD_<seed>_<seq>.png`（失败不阻断发送）。
- [ ] 文本形式（A/B/C）后端读自己的 Game 渲染，前端只发 `format`（不带棋盘数据、不带 model）。

### 接口契约

```ts
// ai/screenshot.ts
export async function captureBoardImage(boardEl: HTMLElement, opts?: { pixelRatio?: number }): Promise<string>;
// 返回 PNG data URL（默认 pixelRatio 不放大）；供 GuideRequest.imageDataUrl；Playwright 只作开发/工具截图，不作 runtime capture
```

后端（S5）：格式 D → `suggest` 切 `set_model(VISION_MODEL="deepseek-v4-flash-vision-exp")`，image blocks 用 `content` 数组 `image_url`（每图 ≤384 token）；
把 `req.image_data_url` 留底写盘 `<exe_dir>/base64_img/YYYYMMDD_<seed>_<seq>.png`（失败不阻断发送）。
