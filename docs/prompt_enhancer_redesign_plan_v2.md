# Prompt Enhancer Redesign Plan v2: Opt-in Auto-Detection with Strict Boundaries

> **修订历史**
> - **v1** (Gemini 撰写): 方向正确但风险评估不足，被 Claude Opus 4.7 + Codex GPT-5.2 双模型交叉审查后判定为 **REJECT**
> - **v2** (本文档): 整合 9 条审查发现，重新设计为「opt-in 自动嗅探 + 严格边界 + 友好降级」三位一体方案
>
> **审稿人**: Claude Opus 4.7 (浮浮酱) + Codex GPT-5.2
> **撰写人**: Claude Opus 4.7
> **撰写日期**: 2026-05-23
> **目标项目**: `F:\claude-tools\ace-tool-rs` (acemcp-rust)

---

## 1. 背景与 v1 plan 的核心缺陷

### 1.1 原始问题
当用户调用 `enhance_prompt` MCP 工具时，由于 `EnhancerEndpoint::Local` 是默认值且其行为是直接 `render_enhance_prompt(original_prompt)` 渲染元模板（以 `⚠️ NO TOOLS ALLOWED ⚠️` 开头）后返回，导致 MCP client 把这段模板当成"增强结果"展示给用户，造成"增强失败 + 诡异警告"的强烈认知断裂。

### 1.2 v1 plan 的致命缺陷（5 处）

| # | 缺陷 | 严重度 | 来源 |
|---|------|-------|------|
| 1 | **隐性数据外发**：默认行为从"离线 Local"翻转为"调用第三方 API"，用户在毫无知情下，prompt + history 被发往 Anthropic/Google/OpenAI | 🔴 P0 | Codex |
| 2 | **Local 兜底依然吐模板**：v1 只改了路由策略，没动 Local 模式本身的行为，所有"无 key"用户依然踩同样的坑 | 🔴 P0 | Claude |
| 3 | **Multi-key 静默歧义**：用户同时配置三个 Key 时，硬编码 `Claude > Gemini > OpenAI` 顺序，可能选错 provider 且无可观测性 | 🟠 P1 | 双方 |
| 4 | **Empty/whitespace 漏洞**：`std::env::var(...).is_ok()` 在空字符串时仍返回 true，会先选中再报错，屏蔽后续可用 provider | 🟠 P1 | 双方 |
| 5 | **Model 字段未对齐**：v1 只解决 key/base_url 的 fallback，hardcoded `DEFAULT_*_MODEL` 仍然可能命中代理无权限的 model | 🟠 P1 | Codex |

### 1.3 v2 设计原则

- **安全优先**：不允许通过环境变量继承造成隐性数据外发，所有联网行为必须显式 opt-in
- **失败可观测**：任何 fallback、选择、降级路径都打 INFO/WARN 日志，方便排查
- **失败不静默**：解析错误、空值、未知值一律 fail-fast，不要静默退到 Local 再吐模板
- **KISS 但完整**：保留 Gemini v1 的简洁框架，但补全测试、observability、错误处理三个维度
- **后向兼容显式声明**：明确列出哪些行为变化属于"行为变更"而非"破坏性变更"

---

## 2. 关键架构决策（ADR）

### ADR-1: Auto-detect 必须显式 opt-in

**决策**：`ACE_ENHANCER_ENDPOINT` 缺省或为空时**仍然 fallback 到 Local**（不出网），用户必须显式设置 `ACE_ENHANCER_ENDPOINT=auto` 才会触发 API Key 嗅探。

**理由**：
- 当前默认行为是离线、不计费、prompt 不外发
- v1 plan 让默认行为变成"任意第三方 API key 存在就静默调用"，构成隐性数据边界翻转
- 显式 opt-in 是行业惯例（如 git config 的 `--global` 显式声明）

**反对意见处理**：
- "这样就不是 zero-config 了" → 实际上 zero-config 本身就是误命题。用户必须主动配置 `ACE_ENHANCER_ENDPOINT=auto`，但配置一次后即可享受多 provider 自动路由
- "用户不愿多配一个环境变量" → 比起静默把 prompt 发给第三方，多输入 4 个字符是更小的代价

---

### ADR-2: Local 模式行为变更 — 不再吐模板

**决策**：`EnhancerEndpoint::Local` 模式下，直接返回原始 prompt（不修饰、不渲染模板），并打 `WARN` 日志告知"未配置增强后端，已透传"。

**理由**：
- 当前 Local 模式吐 `ENHANCE_PROMPT_TEMPLATE` 是把"给 LLM 的指令"误当成"给 MCP client 的结果"，是设计 bug
- 透传原始 prompt 让 MCP client 至少能继续执行任务，符合"优雅降级"
- WARN 日志让用户在 MCP server 日志里能看到原因

**实现**：
```rust
EnhancerEndpoint::Local => {
    warn!(
        "ACE_ENHANCER_ENDPOINT not configured or set to 'local'. \
         Returning original prompt as-is. To enable enhancement, set \
         ACE_ENHANCER_ENDPOINT=auto (with API key inheritance) or \
         ACE_ENHANCER_ENDPOINT=claude/gemini/openai (explicit)."
    );
    Ok(original_prompt.to_string())
}
```

---

### ADR-3: Multi-key 处理策略 — 优先级显式声明 + 默认顺序兜底

**决策**：当用户设置 `ACE_ENHANCER_ENDPOINT=auto` 时：

1. **优先级 1**：读取 `ACE_ENHANCER_PREFERRED_PROVIDER`（用户显式偏好），存在则直接使用
2. **优先级 2**：按 deterministic 顺序 `Claude > Gemini > OpenAI`（与 ATX/SDK 文档惯例一致）扫描，**选第一个非空 key 对应的 provider**
3. **优先级 3**：全部为空 → fallback 到 Local + WARN

**注意**：本决策**没有**采纳 Codex 的"多 key 共存直接报歧义错误"方案，原因：
- 实际用户场景：很多人同时配置三个 key 指向同一个聚合代理（OneAPI / OpenRouter）
- 报错强迫用户每次显式选择不符合 zero-friction 期望
- Deterministic 顺序 + 必打的 observability 日志，让用户至少知道选了谁

**与 Codex 的妥协点**：通过 observability 日志（ADR-7）让"选错 provider"立刻可见，而非静默。

---

### ADR-4: Empty / whitespace env var 严格判空

**决策**：抽取统一 helper `read_nonempty_env(key) -> Option<String>`，所有环境变量读取必须经过此 helper。

**理由**：
- `std::env::var("X").is_ok()` 在 `X=""` 时返回 true，是 Rust 标准库的"已知陷阱"
- 当 `ANTHROPIC_API_KEY=""` 时，v1 plan 会选中 Claude 然后立即报"API token is empty"，把后续可用的 Gemini/OpenAI 屏蔽
- 集中处理消除 6+ 处重复的 `trim` + `is_empty` 检查

**实现**：
```rust
/// Read environment variable, treating empty/whitespace-only values as None
fn read_nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn has_nonempty_env(key: &str) -> bool {
    read_nonempty_env(key).is_some()
}
```

---

### ADR-5: Model 字段 provider 级 fallback 链

**决策**：Model 解析顺序为：
1. `PROMPT_ENHANCER_MODEL`（增强专用，最高优先级）
2. `ANTHROPIC_MODEL` / `GEMINI_MODEL` / `OPENAI_MODEL`（provider 标准变量，与 client SDK 一致）
3. `DEFAULT_CLAUDE_MODEL` / `DEFAULT_GEMINI_MODEL` / `DEFAULT_OPENAI_MODEL`（hardcoded 兜底）

**理由**：
- v1 plan 只解决了 key/base_url 的 fallback，model 仍走 hardcoded 默认
- 用户的 Claude Code 当前可能用 Opus，增强请求却悄悄 fallback 到 Sonnet 4.5，不一致
- 用户的代理（如 OneAPI）可能只开通了某些 model，hardcoded 默认值直接 400

**实现**：
```rust
fn resolve_model(endpoint: EnhancerEndpoint) -> String {
    // Priority 1: enhancer-specific override
    if let Some(m) = read_nonempty_env(ENV_ENHANCER_MODEL) {
        return m;
    }
    // Priority 2: provider standard env vars
    let provider_env = match endpoint {
        EnhancerEndpoint::Claude => Some("ANTHROPIC_MODEL"),
        EnhancerEndpoint::Gemini => Some("GEMINI_MODEL"),
        EnhancerEndpoint::OpenAI => Some("OPENAI_MODEL"),
        _ => None,
    };
    if let Some(env_name) = provider_env {
        if let Some(m) = read_nonempty_env(env_name) {
            return m;
        }
    }
    // Priority 3: hardcoded fallback
    default_model_for(endpoint).to_string()
}

fn default_model_for(endpoint: EnhancerEndpoint) -> &'static str {
    match endpoint {
        EnhancerEndpoint::Claude => DEFAULT_CLAUDE_MODEL,
        EnhancerEndpoint::Gemini => DEFAULT_GEMINI_MODEL,
        EnhancerEndpoint::OpenAI => DEFAULT_OPENAI_MODEL,
        _ => "claude-sonnet-4-5",
    }
}
```

---

### ADR-6: `EnhancerEndpoint::from_env_str` 改为 fallible

**决策**：把 `from_env_str(&str) -> Self` 改为 `try_from_env_str(&str) -> Result<Self>`，未知值返回 `Err`，**不再静默退回 Local**。

**理由**：
- 当前实现：用户写错（`anthropic` / `claude-code` / `gpt`）会静默退 Local，又开始吐模板
- 错误配置应该立即抛错，让用户在 MCP server 启动时就能发现，而不是运行时神秘失败
- 显式声明的 `local` 字符串仍然合法（→ `Ok(Local)`）

**实现**：
```rust
impl EnhancerEndpoint {
    pub fn try_from_env_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "old" => Ok(Self::Old),
            "new" => Ok(Self::New),
            "claude" => Ok(Self::Claude),
            "openai" => Ok(Self::OpenAI),
            "gemini" => Ok(Self::Gemini),
            "auto" => Err(anyhow!(
                "'auto' is a meta value handled by get_enhancer_endpoint(), \
                 not a valid EnhancerEndpoint variant"
            )),
            other => Err(anyhow!(
                "Unsupported ACE_ENHANCER_ENDPOINT value: '{}'. \
                 Valid options: local, claude, gemini, openai, old, new, auto",
                other
            )),
        }
    }
}
```

---

### ADR-7: Observability — Provenance Logging

**决策**：所有配置解析路径必须打 INFO 日志，标注每个值的来源（哪个环境变量）但**绝不打印值本身**（防 token 泄露）。

**理由**：
- v1 plan 完全没考虑日志
- MCP 子进程的环境变量继承在 Windows GUI 启动 / VSCode 内嵌 shell 等场景下行为微妙
- 用户报"为什么我的增强没生效"时，没有日志根本无法 debug

**实现**：
```rust
#[derive(Debug)]
struct ResolvedConfig {
    endpoint: EnhancerEndpoint,
    endpoint_source: &'static str,    // e.g. "ACE_ENHANCER_ENDPOINT=auto -> ANTHROPIC_API_KEY"
    token_source: &'static str,       // e.g. "PROMPT_ENHANCER_TOKEN" or "ANTHROPIC_API_KEY"
    base_url_source: String,          // e.g. "PROMPT_ENHANCER_BASE_URL" or "default (https://api.anthropic.com)"
    model_source: String,             // e.g. "ANTHROPIC_MODEL" or "default (claude-sonnet-4-5-20250929)"
}

info!(
    endpoint = %resolved.endpoint,
    endpoint_source = resolved.endpoint_source,
    token_source = resolved.token_source,
    base_url_source = %resolved.base_url_source,
    model_source = %resolved.model_source,
    "Prompt enhancer provider resolved"
);
```

---

### ADR-8: Provider 响应解析健壮性

**决策**：抽取 `first_nonempty_text()` helper，所有 provider 的响应解析改为「遍历所有 content/candidate/choice，取第一个非空文本」。

**理由**：
- 当前 Gemini/OpenAI 实现强依赖"第一个 candidate/choice 里就有文本"
- 实际场景：Gemini safety block 会让第一个 candidate 的 parts 为空，但后续 candidate 可能有
- 代理兼容层（OneAPI 等）的返回 shape 可能差异巨大

**实现**：
```rust
/// Extract first non-empty trimmed text from a sequence of optional strings
pub fn first_nonempty_text<I>(parts: I) -> Option<String>
where
    I: IntoIterator<Item = Option<String>>,
{
    parts.into_iter()
        .flatten()
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
}
```

---

### ADR-9: Augment 端点（`New`/`Old`）保留但不参与 auto-detect

**决策**：保留 `EnhancerEndpoint::New` 和 `EnhancerEndpoint::Old` 作为显式 endpoint，但 auto-detect 不再考虑 Augment Token。

**理由**：
- 项目方约束：所有 Augment 账号均无 API 额度，调用必失败
- 项目本质是"白嫖" Augment 的代码库索引能力（`search_context`），与 `enhance_prompt` 解耦
- 删除会破坏向后兼容（用户可能还有显式 `ACE_ENHANCER_ENDPOINT=old` 配置）

---

## 3. 实施细节

### 3.1 文件影响范围

| 文件 | 修改类型 | 行数估计 |
|------|---------|---------|
| `src/service/common.rs` | 主要修改：抽 helper + 重写 `get_third_party_config` + `EnhancerEndpoint::try_from_env_str` | +120, -50 |
| `src/enhancer/prompt_enhancer.rs` | 主要修改：重写 `get_enhancer_endpoint` + 改造 Local 分支 | +60, -10 |
| `src/service/gemini.rs` | 小修改：使用 `first_nonempty_text` | +5, -8 |
| `src/service/openai.rs` | 小修改：使用 `first_nonempty_text` | +5, -8 |
| `src/service/claude.rs` | 小修改：使用 `first_nonempty_text` | +5, -8 |
| 新增 `tests/enhancer_routing_test.rs` | 新增：端到端路由测试 | +200 |

### 3.2 新增环境变量速查表

| 变量名 | 用途 | 默认值 | 优先级 |
|--------|------|--------|--------|
| `ACE_ENHANCER_ENDPOINT` | 显式指定 endpoint 或开启 auto | (不设置)→Local | 最高 |
| `ACE_ENHANCER_PREFERRED_PROVIDER` | auto 模式下的偏好 provider | (不设置) | auto 模式下次高 |
| `PROMPT_ENHANCER_TOKEN` | 通用 token override | (不设置) | provider 标准 env 之上 |
| `PROMPT_ENHANCER_BASE_URL` | 通用 base URL override | (不设置) | provider 标准 env 之上 |
| `PROMPT_ENHANCER_MODEL` | 通用 model override | (不设置) | provider 标准 env 之上 |
| `ANTHROPIC_API_KEY` | Claude 标准 key（auto 探测） | (不设置) | provider 标准 |
| `ANTHROPIC_BASE_URL` | Claude 标准 base URL | `https://api.anthropic.com` | provider 标准 |
| `ANTHROPIC_MODEL` | Claude 标准 model | `DEFAULT_CLAUDE_MODEL` | provider 标准 |
| `GEMINI_API_KEY` | Gemini 标准 key | (不设置) | provider 标准 |
| `GEMINI_BASE_URL` | Gemini 标准 base URL | `https://generativelanguage.googleapis.com` | provider 标准 |
| `GEMINI_MODEL` | Gemini 标准 model | `DEFAULT_GEMINI_MODEL` | provider 标准 |
| `OPENAI_API_KEY` | OpenAI 标准 key | (不设置) | provider 标准 |
| `OPENAI_BASE_URL` | OpenAI 标准 base URL | `https://api.openai.com` | provider 标准 |
| `OPENAI_MODEL` | OpenAI 标准 model | `DEFAULT_OPENAI_MODEL` | provider 标准 |

### 3.3 完整代码实施

#### 3.3.1 `src/service/common.rs`

```rust
// ====== Helpers (NEW) ======

/// Read environment variable, treating empty/whitespace-only values as None
fn read_nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn has_nonempty_env(key: &str) -> bool {
    read_nonempty_env(key).is_some()
}

/// Provider-specific defaults: (api_key_env, base_url_env, default_base_url)
fn provider_defaults(endpoint: EnhancerEndpoint) -> Option<(&'static str, &'static str, &'static str)> {
    match endpoint {
        EnhancerEndpoint::Claude => Some((
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "https://api.anthropic.com",
        )),
        EnhancerEndpoint::Gemini => Some((
            "GEMINI_API_KEY",
            "GEMINI_BASE_URL",
            "https://generativelanguage.googleapis.com",
        )),
        EnhancerEndpoint::OpenAI => Some((
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
            "https://api.openai.com",
        )),
        _ => None,
    }
}

fn default_model_for(endpoint: EnhancerEndpoint) -> &'static str {
    match endpoint {
        EnhancerEndpoint::Claude => DEFAULT_CLAUDE_MODEL,
        EnhancerEndpoint::Gemini => DEFAULT_GEMINI_MODEL,
        EnhancerEndpoint::OpenAI => DEFAULT_OPENAI_MODEL,
        _ => "claude-sonnet-4-5",
    }
}

/// Extract first non-empty trimmed text from a sequence of optional strings
pub fn first_nonempty_text<I>(parts: I) -> Option<String>
where
    I: IntoIterator<Item = Option<String>>,
{
    parts.into_iter()
        .flatten()
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
}

// ====== EnhancerEndpoint (MODIFIED) ======

impl EnhancerEndpoint {
    /// Parse from environment variable string (strict, fallible)
    /// Note: 'auto' is NOT a valid variant here; it's handled at the route layer
    pub fn try_from_env_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "old" => Ok(Self::Old),
            "new" => Ok(Self::New),
            "claude" => Ok(Self::Claude),
            "openai" => Ok(Self::OpenAI),
            "gemini" => Ok(Self::Gemini),
            "auto" => Err(anyhow!(
                "'auto' is handled by get_enhancer_endpoint(), not a direct variant"
            )),
            other => Err(anyhow!(
                "Unsupported ACE_ENHANCER_ENDPOINT value: '{}'. \
                 Valid options: local, claude, gemini, openai, old, new, auto",
                other
            )),
        }
    }

    /// Backward-compat shim: keep old infallible version returning Local for unknown
    /// DEPRECATED: prefer try_from_env_str
    #[deprecated(note = "use try_from_env_str for strict parsing")]
    pub fn from_env_str(s: &str) -> Self {
        Self::try_from_env_str(s).unwrap_or(Self::Local)
    }
}

// ====== Provenance Tracking (NEW) ======

#[derive(Debug, Clone)]
pub struct ResolvedThirdPartyConfig {
    pub config: ThirdPartyConfig,
    pub token_source: &'static str,
    pub base_url_source: String,
    pub model_source: String,
}

// ====== get_third_party_config (REWRITTEN) ======

pub fn get_third_party_config(endpoint: EnhancerEndpoint) -> Result<ThirdPartyConfig> {
    Ok(resolve_third_party_config(endpoint)?.config)
}

pub fn resolve_third_party_config(endpoint: EnhancerEndpoint) -> Result<ResolvedThirdPartyConfig> {
    let (api_key_env, base_url_env, default_base_url) = provider_defaults(endpoint)
        .ok_or_else(|| anyhow!("Unsupported endpoint for third-party config: {}", endpoint))?;

    // ---- Token resolution ----
    let (token, token_source) = if let Some(t) = read_nonempty_env(ENV_ENHANCER_TOKEN) {
        (t, ENV_ENHANCER_TOKEN)
    } else if let Some(t) = read_nonempty_env(api_key_env) {
        (t, api_key_env)
    } else {
        return Err(anyhow!(
            "No API token for '{}' endpoint. Set either {} or {}",
            endpoint, ENV_ENHANCER_TOKEN, api_key_env
        ));
    };

    // ---- Base URL resolution ----
    let (base_url, base_url_source) = if let Some(u) = read_nonempty_env(ENV_ENHANCER_BASE_URL) {
        (u, ENV_ENHANCER_BASE_URL.to_string())
    } else if let Some(u) = read_nonempty_env(base_url_env) {
        (u, base_url_env.to_string())
    } else {
        (default_base_url.to_string(), format!("default ({})", default_base_url))
    };
    let base_url = base_url.trim_end_matches('/').to_string();

    // ---- Model resolution ----
    let provider_model_env = match endpoint {
        EnhancerEndpoint::Claude => Some("ANTHROPIC_MODEL"),
        EnhancerEndpoint::Gemini => Some("GEMINI_MODEL"),
        EnhancerEndpoint::OpenAI => Some("OPENAI_MODEL"),
        _ => None,
    };
    let (model, model_source) = if let Some(m) = read_nonempty_env(ENV_ENHANCER_MODEL) {
        (m, ENV_ENHANCER_MODEL.to_string())
    } else if let Some(m) = provider_model_env.and_then(read_nonempty_env) {
        (m, provider_model_env.unwrap().to_string())
    } else {
        let default = default_model_for(endpoint);
        (default.to_string(), format!("default ({})", default))
    };

    Ok(ResolvedThirdPartyConfig {
        config: ThirdPartyConfig { base_url, token, model },
        token_source,
        base_url_source,
        model_source,
    })
}
```

#### 3.3.2 `src/enhancer/prompt_enhancer.rs`

```rust
// ====== Constants (NEW) ======

pub const ENV_ENHANCER_PREFERRED_PROVIDER: &str = "ACE_ENHANCER_PREFERRED_PROVIDER";

// ====== get_enhancer_endpoint (REWRITTEN) ======

/// Provenance info for endpoint selection (for logging)
#[derive(Debug)]
pub struct EndpointDecision {
    pub endpoint: EnhancerEndpoint,
    pub source: String,  // Human-readable explanation
}

pub fn get_enhancer_endpoint() -> Result<EndpointDecision> {
    let raw = read_nonempty_env(ENV_ENHANCER_ENDPOINT);

    match raw.as_deref().map(str::to_lowercase).as_deref() {
        Some("auto") => detect_auto_endpoint(),
        None => Ok(EndpointDecision {
            endpoint: EnhancerEndpoint::Local,
            source: format!("{} not set, defaulting to Local", ENV_ENHANCER_ENDPOINT),
        }),
        Some(other) => {
            let endpoint = EnhancerEndpoint::try_from_env_str(other)?;
            Ok(EndpointDecision {
                endpoint,
                source: format!("{}={}", ENV_ENHANCER_ENDPOINT, other),
            })
        }
    }
}

/// Auto-detection logic for ACE_ENHANCER_ENDPOINT=auto
fn detect_auto_endpoint() -> Result<EndpointDecision> {
    // Priority 1: explicit preferred provider
    if let Some(preferred) = read_nonempty_env(ENV_ENHANCER_PREFERRED_PROVIDER) {
        let endpoint = EnhancerEndpoint::try_from_env_str(&preferred)?;
        let (key_env, _, _) = provider_defaults(endpoint).ok_or_else(|| {
            anyhow!(
                "{}={} but provider has no auto-detect support",
                ENV_ENHANCER_PREFERRED_PROVIDER, preferred
            )
        })?;
        if !has_nonempty_env(key_env) {
            return Err(anyhow!(
                "{}={} specified but {} is not set or empty",
                ENV_ENHANCER_PREFERRED_PROVIDER, preferred, key_env
            ));
        }
        return Ok(EndpointDecision {
            endpoint,
            source: format!(
                "auto + {}={} + {} present",
                ENV_ENHANCER_PREFERRED_PROVIDER, preferred, key_env
            ),
        });
    }

    // Priority 2: deterministic scan order
    let candidates = [
        (EnhancerEndpoint::Claude, "ANTHROPIC_API_KEY"),
        (EnhancerEndpoint::Gemini, "GEMINI_API_KEY"),
        (EnhancerEndpoint::OpenAI, "OPENAI_API_KEY"),
    ];
    for (ep, key) in candidates {
        if has_nonempty_env(key) {
            return Ok(EndpointDecision {
                endpoint: ep,
                source: format!("auto -> {} (first non-empty key)", key),
            });
        }
    }

    // Priority 3: fallback to Local with warning
    warn!(
        "ACE_ENHANCER_ENDPOINT=auto but no provider API key found. \
         Set ANTHROPIC_API_KEY, GEMINI_API_KEY, or OPENAI_API_KEY to enable. \
         Falling back to Local (will return original prompt)."
    );
    Ok(EndpointDecision {
        endpoint: EnhancerEndpoint::Local,
        source: "auto -> no keys found, fallback to Local".to_string(),
    })
}

// ====== call_prompt_enhancer_api_static (MODIFIED) ======

async fn call_prompt_enhancer_api_static(
    client: &Client,
    config: &Config,
    original_prompt: &str,
    conversation_history: &str,
    blob_names: &[String],
) -> Result<String> {
    let decision = get_enhancer_endpoint()?;
    info!(endpoint = %decision.endpoint, source = %decision.source, "Endpoint selected");

    match decision.endpoint {
        EnhancerEndpoint::Local => {
            warn!(
                "Using LOCAL mode: returning original prompt as-is. \
                 To enable enhancement, set ACE_ENHANCER_ENDPOINT=auto or specific provider."
            );
            Ok(original_prompt.to_string())
        }
        EnhancerEndpoint::New => {
            info!("Using NEW prompt-enhancer endpoint (Augment)");
            call_new_endpoint(client, config, original_prompt, conversation_history).await
        }
        EnhancerEndpoint::Old => {
            info!("Using OLD chat-stream endpoint (Augment)");
            call_old_endpoint(client, config, original_prompt, conversation_history, blob_names).await
        }
        EnhancerEndpoint::Claude | EnhancerEndpoint::OpenAI | EnhancerEndpoint::Gemini => {
            let resolved = resolve_third_party_config(decision.endpoint)?;
            info!(
                endpoint = %decision.endpoint,
                token_source = resolved.token_source,
                base_url_source = %resolved.base_url_source,
                model_source = %resolved.model_source,
                "Third-party config resolved"
            );
            match decision.endpoint {
                EnhancerEndpoint::Claude => call_claude_endpoint(client, &resolved.config, original_prompt, conversation_history).await,
                EnhancerEndpoint::OpenAI => call_openai_endpoint(client, &resolved.config, original_prompt, conversation_history).await,
                EnhancerEndpoint::Gemini => call_gemini_endpoint(client, &resolved.config, original_prompt, conversation_history).await,
                _ => unreachable!(),
            }
        }
    }
}
```

#### 3.3.3 `src/service/gemini.rs` / `openai.rs` / `claude.rs` (片段示例)

```rust
// gemini.rs - 替换 lines 136-141
let text = first_nonempty_text(
    api_response.candidates.into_iter().flat_map(|c| {
        c.content.parts.into_iter().map(|p| p.text)
    })
).ok_or_else(|| anyhow!("Gemini API returned no usable text. Possible safety block or non-text response. Body: {}", body_text))?;
```

---

## 4. 测试策略

### 4.1 设计原则
- **不污染进程 env**：测试通过依赖注入（trait/closure）传入"假"的 env getter，避免多线程并发改 `std::env` 导致的不稳定
- **覆盖优先级矩阵**：所有 env 变量组合的优先级关系
- **覆盖失败路径**：empty string、unknown value、missing key 等

### 4.2 注入式 helper（重构 enabler）

```rust
/// Pure-function version for testability
pub fn resolve_endpoint_with<F>(env_get: F) -> Result<EndpointDecision>
where
    F: Fn(&str) -> Option<String>,
{
    // ... same logic as get_enhancer_endpoint but using env_get instead of std::env::var
}

// Production wrapper
pub fn get_enhancer_endpoint() -> Result<EndpointDecision> {
    resolve_endpoint_with(|k| read_nonempty_env(k))
}
```

### 4.3 关键测试用例（新文件 `src/enhancer/routing_tests.rs`）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + '_ {
        let map: HashMap<String, String> = pairs.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).filter(|v| !v.trim().is_empty()).cloned()
    }

    #[test]
    fn unset_endpoint_defaults_to_local() {
        let env = env_from(&[]);
        let d = resolve_endpoint_with(env).unwrap();
        assert_eq!(d.endpoint, EnhancerEndpoint::Local);
    }

    #[test]
    fn empty_endpoint_defaults_to_local() {
        let env = env_from(&[("ACE_ENHANCER_ENDPOINT", "")]);
        let d = resolve_endpoint_with(env).unwrap();
        assert_eq!(d.endpoint, EnhancerEndpoint::Local);
    }

    #[test]
    fn auto_with_no_keys_falls_back_to_local() {
        let env = env_from(&[("ACE_ENHANCER_ENDPOINT", "auto")]);
        let d = resolve_endpoint_with(env).unwrap();
        assert_eq!(d.endpoint, EnhancerEndpoint::Local);
        assert!(d.source.contains("no keys"));
    }

    #[test]
    fn auto_picks_anthropic_when_only_claude_key_set() {
        let env = env_from(&[
            ("ACE_ENHANCER_ENDPOINT", "auto"),
            ("ANTHROPIC_API_KEY", "sk-ant-test"),
        ]);
        let d = resolve_endpoint_with(env).unwrap();
        assert_eq!(d.endpoint, EnhancerEndpoint::Claude);
    }

    #[test]
    fn auto_prefers_claude_in_default_order() {
        let env = env_from(&[
            ("ACE_ENHANCER_ENDPOINT", "auto"),
            ("ANTHROPIC_API_KEY", "sk-ant-test"),
            ("OPENAI_API_KEY", "sk-test"),
            ("GEMINI_API_KEY", "AIzaTest"),
        ]);
        let d = resolve_endpoint_with(env).unwrap();
        assert_eq!(d.endpoint, EnhancerEndpoint::Claude);  // deterministic order
    }

    #[test]
    fn preferred_provider_overrides_default_order() {
        let env = env_from(&[
            ("ACE_ENHANCER_ENDPOINT", "auto"),
            ("ACE_ENHANCER_PREFERRED_PROVIDER", "openai"),
            ("ANTHROPIC_API_KEY", "sk-ant-test"),
            ("OPENAI_API_KEY", "sk-test"),
        ]);
        let d = resolve_endpoint_with(env).unwrap();
        assert_eq!(d.endpoint, EnhancerEndpoint::OpenAI);
    }

    #[test]
    fn preferred_provider_without_key_errors() {
        let env = env_from(&[
            ("ACE_ENHANCER_ENDPOINT", "auto"),
            ("ACE_ENHANCER_PREFERRED_PROVIDER", "openai"),
            ("ANTHROPIC_API_KEY", "sk-ant-test"),
            // OPENAI_API_KEY missing
        ]);
        let result = resolve_endpoint_with(env);
        assert!(result.is_err());
    }

    #[test]
    fn whitespace_only_key_treated_as_unset() {
        let env = env_from(&[
            ("ACE_ENHANCER_ENDPOINT", "auto"),
            ("ANTHROPIC_API_KEY", "   "),
            ("OPENAI_API_KEY", "sk-test"),
        ]);
        let d = resolve_endpoint_with(env).unwrap();
        assert_eq!(d.endpoint, EnhancerEndpoint::OpenAI);  // Anthropic skipped
    }

    #[test]
    fn unknown_endpoint_value_errors() {
        let env = env_from(&[("ACE_ENHANCER_ENDPOINT", "anthropic")]);  // wrong name
        let result = resolve_endpoint_with(env);
        assert!(result.is_err());
        assert!(format!("{:?}", result.unwrap_err()).contains("Unsupported"));
    }

    #[test]
    fn explicit_endpoint_overrides_auto_detect() {
        let env = env_from(&[
            ("ACE_ENHANCER_ENDPOINT", "gemini"),
            ("ANTHROPIC_API_KEY", "sk-ant-test"),  // would auto-pick this, but explicit wins
        ]);
        let d = resolve_endpoint_with(env).unwrap();
        assert_eq!(d.endpoint, EnhancerEndpoint::Gemini);
    }

    #[test]
    fn local_mode_explicit() {
        let env = env_from(&[
            ("ACE_ENHANCER_ENDPOINT", "local"),
            ("ANTHROPIC_API_KEY", "sk-ant-test"),  // should not trigger auto
        ]);
        let d = resolve_endpoint_with(env).unwrap();
        assert_eq!(d.endpoint, EnhancerEndpoint::Local);
    }
}
```

### 4.4 `first_nonempty_text` 测试
```rust
#[test]
fn skips_none_and_empty() {
    let parts = vec![None, Some("".to_string()), Some("  ".to_string()), Some("real".to_string())];
    assert_eq!(first_nonempty_text(parts), Some("real".to_string()));
}

#[test]
fn returns_none_when_all_empty() {
    let parts: Vec<Option<String>> = vec![None, Some("".to_string())];
    assert_eq!(first_nonempty_text(parts), None);
}
```

---

## 5. 向后兼容性与迁移

### 5.1 行为变更明细

| 场景 | 旧行为 | 新行为 | 类型 |
|------|--------|--------|------|
| `ACE_ENHANCER_ENDPOINT` 未设置 | 调 Local，返回 `⚠️ NO TOOLS ALLOWED ⚠️` 模板 | 调 Local，返回原始 prompt + WARN | **Bug fix**（行为变更但修复诡异输出） |
| `ACE_ENHANCER_ENDPOINT=local` 显式 | 同上（吐模板） | 返回原始 prompt + WARN | 同上 |
| `ACE_ENHANCER_ENDPOINT=claude` + `PROMPT_ENHANCER_TOKEN` 缺失 | 报错 | 尝试 fallback 到 `ANTHROPIC_API_KEY` | **行为变更**（新增 fallback） |
| `ACE_ENHANCER_ENDPOINT=auto` | 等同 Local（未知值静默退回） | 触发 auto-detect | **新增功能** |
| `ACE_ENHANCER_ENDPOINT=anthropic` (拼错) | 静默退 Local | **报错**：unsupported value | **破坏性变更**（但实际是 bug fix） |
| 所有 v1/v2 显式 `claude`/`gemini`/`openai`/`old`/`new` | 不变 | 不变 | 完全兼容 |

### 5.2 用户迁移指南（写入 README）

```markdown
### 升级到 v2 后的推荐配置

**场景 A：完全本地，不联网（默认行为）**
```bash
# 不需要设置任何变量，或显式：
unset ACE_ENHANCER_ENDPOINT
# 或：export ACE_ENHANCER_ENDPOINT=local
```
增强工具会透传原始 prompt（不再吐 "NO TOOLS ALLOWED" 模板）。

**场景 B：自动跟随当前 shell 的 API key**
```bash
export ACE_ENHANCER_ENDPOINT=auto
# 假设你已有：
export ANTHROPIC_API_KEY=sk-ant-xxx
```
工具会自动选 Claude 端点。

**场景 C：多 key 共存，指定偏好**
```bash
export ACE_ENHANCER_ENDPOINT=auto
export ACE_ENHANCER_PREFERRED_PROVIDER=gemini
export ANTHROPIC_API_KEY=...
export GEMINI_API_KEY=...
```
即使有 ANTHROPIC_API_KEY，也强制走 Gemini。

**场景 D：完全自定义（旧用户）**
```bash
export ACE_ENHANCER_ENDPOINT=claude
export PROMPT_ENHANCER_TOKEN=sk-ant-xxx
export PROMPT_ENHANCER_BASE_URL=https://my-proxy.com
export PROMPT_ENHANCER_MODEL=claude-opus-4-7
```
完全保持向后兼容。
```

---

## 6. 实施阶段（Rollout Phases）

### Phase 1: 基础设施（无副作用）
- 在 `common.rs` 中添加 `read_nonempty_env`、`has_nonempty_env`、`provider_defaults`、`default_model_for`、`first_nonempty_text`、`ResolvedThirdPartyConfig`
- 添加 `EnhancerEndpoint::try_from_env_str`（保留 `from_env_str` 标记 deprecated）
- 不修改任何调用点
- **验证**：`cargo build` 通过，旧测试全绿

### Phase 2: 解析逻辑重写
- 修改 `get_third_party_config` → 调用 `resolve_third_party_config` 取 `.config`
- 新增 `resolve_third_party_config` 完整实现
- 修改 `get_enhancer_endpoint` 返回 `Result<EndpointDecision>`
- 修改 `call_prompt_enhancer_api_static` 处理新返回类型
- **验证**：手动测试 6 个 env var 组合

### Phase 3: 行为改造
- Local 模式改为返回 `original_prompt`
- 加入 observability 日志
- 替换 Gemini/OpenAI/Claude 的响应解析为 `first_nonempty_text`
- **验证**：MCP client 调 enhance_prompt，确认不再吐"NO TOOLS ALLOWED"

### Phase 4: 测试 + 文档
- 添加 `src/enhancer/routing_tests.rs` 含 10+ 测试用例
- 更新 `README.md` 加入"升级指南"
- 更新 `CHANGELOG.md`
- **验证**：`cargo test` 全绿，覆盖率达 80%+（针对新增代码）

### Phase 5: 集成验证
- 在四种 shell 环境（Bash/PowerShell/Cmd/Fish）启动 MCP server，验证 env 继承
- 在 Claude Code、Cursor、Continue 三种 MCP client 中实测
- 验证日志输出符合预期（不泄露 token 值）

---

## 7. 重新风险评估

| 维度 | v1 评估 | v2 评估 | 说明 |
|------|---------|---------|------|
| 默认行为变化 | "fully preserved" ❌ | **明确变更**（local 不再吐模板，但仍不出网） | v2 显式声明 |
| 数据外发风险 | 未提及 ❌ | **零风险**（auto 必须 opt-in） | ADR-1 锁定 |
| Multi-key 歧义 | 未处理 ❌ | **可控**（PREFERRED + deterministic order + logging） | ADR-3 |
| 测试覆盖 | 无 ❌ | **80%+**（注入式测试 10+ 用例） | §4 |
| 可观测性 | 无 ❌ | **完整 provenance log** | ADR-7 |
| 配置错误处理 | 静默 ❌ | **fail-fast** | ADR-6 |
| 实施风险 | LOW（实际 HIGH） | **MEDIUM**（诚实评估） | 行为变更需要充分验证 |

---

## 8. 附录

### 8.1 错误码映射表

| 错误场景 | 错误消息模板 | 处理建议 |
|---------|------------|---------|
| `ACE_ENHANCER_ENDPOINT` 值未知 | `Unsupported ACE_ENHANCER_ENDPOINT value: 'X'. Valid: ...` | 用户检查拼写 |
| `auto` + `PREFERRED=claude` + 无 ANTHROPIC_API_KEY | `ACE_ENHANCER_PREFERRED_PROVIDER=claude specified but ANTHROPIC_API_KEY is not set or empty` | 用户配置 key 或换 PREFERRED |
| 显式 endpoint + 无任何 token | `No API token for 'X' endpoint. Set either PROMPT_ENHANCER_TOKEN or ANTHROPIC_API_KEY` | 提供二选一指引 |
| Provider 响应无 text | `Gemini API returned no usable text. Possible safety block or non-text response. Body: ...` | 包含 body 用于调试 |

### 8.2 决策审计追踪（Decision Audit Trail）

每条 ADR 对应的 review 来源：

- ADR-1 (opt-in): Codex P0
- ADR-2 (Local 行为): Claude P0-1
- ADR-3 (multi-key): Claude P1-4 + Codex P1-1 妥协方案
- ADR-4 (empty env): Claude P1-3 + Codex P1-2 一致
- ADR-5 (model fallback): Codex P1-3
- ADR-6 (fallible parse): Codex P1-4
- ADR-7 (observability): Claude P1-6 + Codex P2-2 强化
- ADR-8 (response robustness): Codex P2-3
- ADR-9 (Augment 保留): 项目方约束

---

**END OF DOCUMENT**

下一步：按 Phase 1-5 顺序实施。每个 Phase 完成后运行 `cargo test` + `cargo clippy` 验证。
