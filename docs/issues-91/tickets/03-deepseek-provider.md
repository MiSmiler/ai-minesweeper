# 03: deepseek provider 真实实现

**What to build:** 真实 DeepSeek（OpenAI 兼容 `POST /chat/completions`）：SSE 流式 `content`/`reasoning_content` 分离、vision `image_url`、错误码映射到 #97 bucket（`Config`/`Upstream`）、`list_models`/`validate_model`。`--test-ai-chat` 走真实网络完整跑通。

**Coverage seams:** S3（`deepseek` 实现）

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] `DeepSeek::new(config)` 纯构造、不发网络请求；`stream_chat` 发真实请求，`Ok(content/reasoning delta + Done)`；连接失败/超时（无 HTTP 码）→ `Err(ProviderError{kind:Upstream, code:None})`。
- [ ] 错误码映射：400/401/402/422 → `Config`，429/500/503 → `Upstream`；`parse_http_error` 正确归类。
- [ ] `validate_model` 校验模型存在（首次经 `list_models` lazily 拉取并缓存，失败可重试）；model 不在本 provider 列表 → `Err(Config)`。
- [ ] `--test-ai-chat <str>` 真实调 DeepSeek 并打印完整回复（`content` 及 `reasoning_content`）。
- [ ] vision：image 形式经 `content` 数组 `image_url` 传图（每图 ≤384 token）。
- [ ] `Provider::stream_chat` 入口先 `validate_model(&req.model)`；`model` 由 `Agent` 填入的 `current_model`（不写死 enum、无我们维护的映射表）。
