# 07: 棋盘行列号轴标（ai/axis）

**What to build:** 棋盘外圈 0-based 行列号 `createBoardAxis` overlay：`.board` 外绝对定位、`pointer-events:none`、**不进 `.board` 截图**、对 4 种 AI 输入形式**零影响**；「行列号」checkbox（默认关、切走即丢、不持久化）。


**Blocked by:** 05

**Status:** ready-for-agent

- [ ] `createBoardAxis(boardEl, {visible})` 渲染外圈 row/col 轴标（0-based）；`setRowsCols`/`setVisible`/`destroy` 可用。
- [ ] overlay 绝对定位、`pointer-events:none`、不进 `.board` 截图。
- [ ] checkbox「行列号」默认关；切 mode 重置、不持久化；**纯视觉**——不动当前局、不清历史、不发不同输入、不弹窗。
- [ ] 开不开轴标，发送给 AI 的内容（4 形式）完全不变。

### 接口契约

```ts
// ai/axis.ts
export interface AxisOverlay {
  setRowsCols(rows: number, cols: number): void;
  setVisible(visible: boolean): void;
  destroy(): void;
}
export function createBoardAxis(boardEl: HTMLElement, opts?: { visible?: boolean }): AxisOverlay;
// .board 外绝对定位 overlay、pointer-events:none、不进 .board 截图、对 4 形式零影响；默认关、切走即丢、不持久化
```
