# 01: 后端 AI 运行时内核（protocol / provider / agent）

**What to build:** 建立通用 AI 运行时（`ai` 模块，与 `core` 解耦）：与供应商解耦的共享值类型、可插拔的 `Provider` seam、`Agent`/`Session` 骨架。`--test-ai-chat` CLI 自检入口先用 mock Provider 验证「发一条 User 消息 → 回一条 Assistant 回复」的单轮 `complete_once` 路径。本 ticket **不接扫雷、不接真实 HTTP**；DeepSeek 真实实现留 03。

**Coverage seams:** S2（protocol）、S3（Provider seam）、S4（agent）

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 用 mock `Provider` 构造 `Agent`，`complete_once` 发一条 User 消息并返回一条 Assistant 回复（含 `content`，可有 `reasoning_content`），报文吻合 `ChatRequest` 契约。
- [ ] `--test-ai-chat <str>` CLI 入口：仅能单独指定、与其它参数互斥（conflicts_with_all）；命中即早退、复用 `complete_once`；无 AI 配置（API key）时明确报错，不进正常产品流程。
- [ ] 共享值类型（`Message`/`ChatRequest`/`StreamChunk`/`ProviderError`/`ContentBlock`/`ToolCall`/`ToolDecl`）存在；`Message` 按 `role` 序列化成 wire 形状，`ProviderError` 序列化为 `{kind,code,message}`。
- [ ] `ProviderStream`（`Pin<Box<dyn Stream<Item=Result<StreamChunk,ProviderError>> + Send>>`）存在；`Provider::stream_chat(req, cancel) -> Result<ProviderStream, ProviderError>` 签名就绪，支持 `cancel`（`CancellationToken`）取消上游。
- [ ] `Agent` 提供 `stream`（流式）与 `complete_once`（聚合，内部复用 stream）；`set_model`/`current_model`/`current_provider` 可用，`ChatRequest.model` 由 `Agent` 填 `current_model`。
