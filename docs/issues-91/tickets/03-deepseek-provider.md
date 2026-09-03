# 03: deepseek provider 真实实现

**What to build:** 真实 DeepSeek（OpenAI 兼容 `POST /chat/completions`）：SSE 流式 `content`/`reasoning_content` 分离、vision `image_url`、错误码映射到 #97 bucket（`Config`/`Upstream`）、`list_models`/`validate_model`。`--test-ai-chat` 走真实网络完整跑通。


**Blocked by:** 01

**Status:** ready-for-agent

- [ ] `DeepSeek::new(config)` 纯构造、不发网络请求；`stream_chat` 发真实请求，`Ok(content/reasoning delta + Done)`；连接失败/超时（无 HTTP 码）→ `Err(ProviderError{kind:Upstream, code:None})`。
- [ ] 错误码映射：400/401/402/422 → `Config`，429/500/503 → `Upstream`；`parse_http_error` 正确归类。
- [ ] `validate_model` 校验模型存在（首次经 `list_models` lazily 拉取并缓存，失败可重试）；model 不在本 provider 列表 → `Err(Config)`。
- [ ] `--test-ai-chat <str>` 真实调 DeepSeek 并打印完整回复（`content` 及 `reasoning_content`）。
- [ ] vision：image 形式经 `content` 数组 `image_url` 传图（每图 ≤384 token）。
- [ ] `Provider::stream_chat` 入口先 `validate_model(&req.model)`；`model` 由 `Agent` 填入的 `current_model`（不写死 enum、无我们维护的映射表）。

### 接口契约

```rust
// ai::provider::deepseek
struct DeepSeek { api_key: String, base_url: String, client: reqwest::Client, models: tokio::sync::OnceCell<Vec<String>> }
struct DeepSeekConfig { api_key: String, base_url: String }
impl DeepSeek {
  fn new(config: DeepSeekConfig) -> Self;   // 纯构造，不发网络请求；provider 只读、不自持 model
  async fn list_models(&self) -> Result<Vec<String>, ProviderError>;   // lazily：OnceCell get_or_try_init，失败可重试
  async fn validate_model(&self, model: &str) -> Result<(), ProviderError>;   // 首次经 list_models 拉取/缓存
  fn parse_http_error(code: u16, message: String) -> ProviderError;   // 400/401/402/422→Config；429/500/503→Upstream
}
impl Provider for DeepSeek {
  // stream_chat 入口先 validate_model(&req.model)，校验通过发请求（req.model = Agent 填的 current_model）
  // 连接失败/超时（无 HTTP 码）→ ProviderError{ kind: Upstream, code: None, message }
}
```
