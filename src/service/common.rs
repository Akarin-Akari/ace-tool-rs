//! Common types and utilities for service modules

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::enhancer::templates::ENHANCE_PROMPT_TEMPLATE;

/// Environment variable for custom prompt enhancer base URL
pub const ENV_ENHANCER_BASE_URL: &str = "PROMPT_ENHANCER_BASE_URL";

/// Environment variable for custom prompt enhancer auth token
pub const ENV_ENHANCER_TOKEN: &str = "PROMPT_ENHANCER_TOKEN";

/// Environment variable for custom prompt enhancer model
pub const ENV_ENHANCER_MODEL: &str = "PROMPT_ENHANCER_MODEL";

/// Default models for third-party APIs
pub const DEFAULT_CLAUDE_MODEL: &str = "claude-sonnet-4-5";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-5.2";
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-3-flash-preview";
pub const DEFAULT_CODEX_MODEL: &str = "gpt-5.3-codex";

// ====== Phase 1 Helpers (ADR-4, ADR-8) ======

/// Read environment variable, treating empty/whitespace-only values as `None`
///
/// This is the canonical way to read env vars throughout the enhancer codebase.
/// It addresses the well-known `std::env::var(...).is_ok()` trap where empty
/// strings (`X=""`) still return `Ok`, which would otherwise short-circuit
/// later fallback logic.
pub fn read_nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Returns `true` if the environment variable is set to a non-empty,
/// non-whitespace value. See [`read_nonempty_env`] for details.
pub fn has_nonempty_env(key: &str) -> bool {
    read_nonempty_env(key).is_some()
}

/// Extract the first non-empty trimmed text from a sequence of optional strings.
///
/// Useful for parsing provider responses that may contain multiple
/// candidates/choices/parts, where the "primary" one might be empty
/// (e.g. Gemini safety blocks producing empty first candidate).
pub fn first_nonempty_text<I>(parts: I) -> Option<String>
where
    I: IntoIterator<Item = Option<String>>,
{
    parts
        .into_iter()
        .flatten()
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
}

/// Enhancer endpoint type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnhancerEndpoint {
    /// Local mode: render template and return directly, no external API call (default)
    Local,
    /// Use Augment /prompt-enhancer endpoint
    New,
    /// Use Augment /chat-stream endpoint
    Old,
    /// Use Claude API (Anthropic)
    Claude,
    /// Use OpenAI API
    OpenAI,
    /// Use Gemini API (Google)
    Gemini,
    /// Use Codex API (OpenAI Responses API)
    Codex,
}

impl std::fmt::Display for EnhancerEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::New => write!(f, "new"),
            Self::Old => write!(f, "old"),
            Self::Claude => write!(f, "claude"),
            Self::OpenAI => write!(f, "openai"),
            Self::Gemini => write!(f, "gemini"),
            Self::Codex => write!(f, "codex"),
        }
    }
}

impl EnhancerEndpoint {
    /// Parse from environment variable string (strict, fallible).
    ///
    /// Returns `Err` for unknown values, including `auto` (which is a meta
    /// value handled at the route layer in `get_enhancer_endpoint`, not a
    /// direct variant). This is the recommended parser; prefer it over the
    /// deprecated infallible [`from_env_str`].
    ///
    /// See ADR-6 in `docs/prompt_enhancer_redesign_plan_v2.md`.
    pub fn try_from_env_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "old" => Ok(Self::Old),
            "new" => Ok(Self::New),
            "claude" => Ok(Self::Claude),
            "openai" => Ok(Self::OpenAI),
            "gemini" => Ok(Self::Gemini),
            "codex" => Ok(Self::Codex),
            "auto" => Err(anyhow!(
                "'auto' is a meta value handled by get_enhancer_endpoint(), \
                 not a valid EnhancerEndpoint variant"
            )),
            other => Err(anyhow!(
                "Unsupported ACE_ENHANCER_ENDPOINT value: '{}'. \
                 Valid options: local, claude, gemini, openai, old, new, codex, auto",
                other
            )),
        }
    }

    /// Parse from environment variable string (infallible, legacy).
    ///
    /// Unknown values silently fall back to `Local`. This preserves
    /// pre-v2 behavior for any caller that hasn't migrated yet.
    #[deprecated(
        since = "0.1.11",
        note = "Unknown values silently degrade to Local; prefer `try_from_env_str` for fail-fast parsing."
    )]
    pub fn from_env_str(s: &str) -> Self {
        Self::try_from_env_str(s).unwrap_or(Self::Local)
    }

    /// Check if this is a third-party API (Claude/OpenAI/Gemini)
    pub fn is_third_party(&self) -> bool {
        matches!(
            self,
            Self::Claude | Self::OpenAI | Self::Gemini | Self::Codex
        )
    }
}

/// Configuration for third-party API endpoints
#[derive(Debug, Clone)]
pub struct ThirdPartyConfig {
    pub base_url: String,
    pub token: String,
    pub model: String,
}

// ====== Phase 1 Helpers: provider defaults (ADR-5) ======

/// Provider-specific environment variable names and default base URL.
///
/// Returns `(api_key_env, base_url_env, default_base_url)` for the three
/// third-party providers; returns `None` for Local/New/Old (which don't
/// participate in the third-party config flow).
pub fn provider_defaults(
    endpoint: EnhancerEndpoint,
) -> Option<(&'static str, &'static str, &'static str)> {
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
        EnhancerEndpoint::Codex => Some((
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
            "https://api.openai.com",
        )),
        _ => None,
    }
}

/// Hardcoded fallback model for a given provider.
///
/// This is the lowest-priority entry in the model fallback chain
/// (PROMPT_ENHANCER_MODEL > provider env chain > this).
pub fn default_model_for(endpoint: EnhancerEndpoint) -> &'static str {
    match endpoint {
        EnhancerEndpoint::Claude => DEFAULT_CLAUDE_MODEL,
        EnhancerEndpoint::Gemini => DEFAULT_GEMINI_MODEL,
        EnhancerEndpoint::OpenAI => DEFAULT_OPENAI_MODEL,
        EnhancerEndpoint::Codex => DEFAULT_CODEX_MODEL,
        _ => "claude-sonnet-4-5",
    }
}

/// Provider-specific environment variable chain for model resolution.
///
/// Returned in priority order — the first non-empty value wins. Each
/// chain starts with the Anthropic/Gemini/OpenAI standard name, then
/// (for Claude) falls back to Claude Code's private `ANTHROPIC_DEFAULT_*`
/// variants. The `_MODEL` variant (with `[1M]` suffix on relays like
/// anyrouter) is preferred over the bare `_MODEL_NAME` variant so that
/// users of 1M-context relays get the right model name without extra
/// configuration.
///
/// Priority for Claude (per ADR-5 amendment 2026-05-22):
///   1. `ANTHROPIC_MODEL`                       — Anthropic standard
///   2. `ANTHROPIC_DEFAULT_OPUS_MODEL`          — Claude Code private, `[1M]` suffix
///   3. `ANTHROPIC_DEFAULT_OPUS_MODEL_NAME`     — Claude Code private, no suffix
///   4. `ANTHROPIC_DEFAULT_SONNET_MODEL`
///   5. `ANTHROPIC_DEFAULT_SONNET_MODEL_NAME`
///   6. `ANTHROPIC_DEFAULT_HAIKU_MODEL`
///   7. `ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME`
///
/// Opus is ranked above Sonnet/Haiku to match relay deployments that
/// only provision the top tier (e.g. anyrouter as of 2026-05). End users
/// can still override the entire chain via `PROMPT_ENHANCER_MODEL`.
pub fn provider_model_env_chain(endpoint: EnhancerEndpoint) -> &'static [&'static str] {
    match endpoint {
        EnhancerEndpoint::Claude => &[
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
        ],
        EnhancerEndpoint::Gemini => &["GEMINI_MODEL"],
        EnhancerEndpoint::OpenAI => &["OPENAI_MODEL"],
        EnhancerEndpoint::Codex => &["OPENAI_MODEL"],
        _ => &[],
    }
}

/// Third-party config bundled with provenance metadata for observability.
///
/// Each `*_source` field is a human-readable string describing where the
/// corresponding value originated (e.g. `"PROMPT_ENHANCER_TOKEN"` or
/// `"default (https://api.anthropic.com)"`). The actual values themselves
/// (tokens, URLs) MUST NOT be logged — only the source labels.
///
/// See ADR-7 in `docs/prompt_enhancer_redesign_plan_v2.md`.
#[derive(Debug, Clone)]
pub struct ResolvedThirdPartyConfig {
    pub config: ThirdPartyConfig,
    pub token_source: &'static str,
    pub base_url_source: String,
    pub model_source: String,
}

/// Get third-party API configuration from environment variables.
///
/// Thin wrapper around [`resolve_third_party_config`] that discards
/// provenance metadata. Prefer the resolved variant when you need to
/// log where each value came from.
pub fn get_third_party_config(endpoint: EnhancerEndpoint) -> Result<ThirdPartyConfig> {
    Ok(resolve_third_party_config(endpoint)?.config)
}

/// Resolve third-party API configuration with provenance metadata.
///
/// Resolution order:
/// - **Token**: `PROMPT_ENHANCER_TOKEN` > provider-standard env (e.g. `ANTHROPIC_API_KEY`)
/// - **Base URL**: `PROMPT_ENHANCER_BASE_URL` > `ANTHROPIC_BASE_URL`/`GEMINI_BASE_URL`/`OPENAI_BASE_URL` > hardcoded default
/// - **Model**: `PROMPT_ENHANCER_MODEL` > `ANTHROPIC_MODEL`/`GEMINI_MODEL`/`OPENAI_MODEL` > hardcoded default
///
/// All env reads go through [`read_nonempty_env`] so empty/whitespace
/// values are treated as unset (ADR-4).
///
/// Returns `Err` for unsupported endpoints (Local/New/Old) or when no
/// token can be found.
pub fn resolve_third_party_config(
    endpoint: EnhancerEndpoint,
) -> Result<ResolvedThirdPartyConfig> {
    let (api_key_env, base_url_env, default_base_url) = provider_defaults(endpoint)
        .ok_or_else(|| {
            anyhow!(
                "Endpoint '{}' is not a third-party provider (Claude/Gemini/OpenAI)",
                endpoint
            )
        })?;

    // ---- Token resolution ----
    let (token, token_source) = if let Some(t) = read_nonempty_env(ENV_ENHANCER_TOKEN) {
        (t, ENV_ENHANCER_TOKEN)
    } else if let Some(t) = read_nonempty_env(api_key_env) {
        (t, api_key_env)
    } else {
        return Err(anyhow!(
            "No API token available for '{}' endpoint. Set either {} (generic override) \
             or {} (provider-standard env var).",
            endpoint,
            ENV_ENHANCER_TOKEN,
            api_key_env
        ));
    };

    // ---- Base URL resolution ----
    let (base_url, base_url_source) = if let Some(u) = read_nonempty_env(ENV_ENHANCER_BASE_URL) {
        (u, ENV_ENHANCER_BASE_URL.to_string())
    } else if let Some(u) = read_nonempty_env(base_url_env) {
        (u, base_url_env.to_string())
    } else {
        (
            default_base_url.to_string(),
            format!("default ({})", default_base_url),
        )
    };
    let base_url = base_url.trim_end_matches('/').to_string();

    // ---- Model resolution ----
    // Per ADR-5 amendment (2026-05-22): walk the full provider env chain
    // so that Claude Code's private `ANTHROPIC_DEFAULT_*_MODEL[_NAME]`
    // variants are picked up automatically. See `provider_model_env_chain`.
    let (model, model_source) = if let Some(m) = read_nonempty_env(ENV_ENHANCER_MODEL) {
        (m, ENV_ENHANCER_MODEL.to_string())
    } else {
        let chain = provider_model_env_chain(endpoint);
        let found = chain
            .iter()
            .find_map(|key| read_nonempty_env(key).map(|v| (v, *key)));
        match found {
            Some((m, key)) => (m, key.to_string()),
            None => {
                let default = default_model_for(endpoint);
                (default.to_string(), format!("default ({})", default))
            }
        }
    };

    Ok(ResolvedThirdPartyConfig {
        config: ThirdPartyConfig {
            base_url,
            token,
            model,
        },
        token_source,
        base_url_source,
        model_source,
    })
}

/// Chat message for conversation history
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Parse conversation history into ChatMessage format
pub fn parse_chat_history(conversation_history: &str) -> Vec<ChatMessage> {
    let mut chat_history = Vec::new();
    let mut current_role: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in conversation_history.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if current_role.is_some() {
                current_lines.push(String::new());
            }
            continue;
        }

        if let Some((role, content)) = parse_history_line(trimmed) {
            if let Some(prev_role) = current_role.take() {
                chat_history.push(ChatMessage {
                    role: prev_role,
                    content: current_lines.join("\n"),
                });
            }
            current_role = Some(role);
            current_lines.clear();
            current_lines.push(content);
        } else if current_role.is_some() {
            current_lines.push(line.to_string());
        }
    }

    if let Some(role) = current_role {
        chat_history.push(ChatMessage {
            role,
            content: current_lines.join("\n"),
        });
    }

    chat_history
}

fn parse_history_line(line: &str) -> Option<(String, String)> {
    let user_prefixes = ["User:", "用户:"];
    for prefix in user_prefixes {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(("user".to_string(), rest.trim().to_string()));
        }
    }

    let assistant_prefixes = ["AI:", "Assistant:", "助手:"];
    for prefix in assistant_prefixes {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(("assistant".to_string(), rest.trim().to_string()));
        }
    }

    None
}

/// Extract enhanced prompt from XML-like response
/// Looks for content between <augment-enhanced-prompt> and </augment-enhanced-prompt> tags
pub fn extract_enhanced_prompt(text: &str) -> Option<String> {
    lazy_static::lazy_static! {
        static ref TAG_RE: Regex = Regex::new(
            r"(?s)<augment-enhanced-prompt(?:\s+[^>]*)?>\s*(.*?)\s*</augment-enhanced-prompt\s*>"
        ).unwrap();
    }

    TAG_RE.captures(text).and_then(|caps| {
        let trimmed = caps.get(1)?.as_str().trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Detect if text is primarily Chinese
pub fn is_chinese_text(text: &str) -> bool {
    lazy_static::lazy_static! {
        static ref CHINESE_RE: Regex = Regex::new(r"[\u4e00-\u9fa5]").unwrap();
    }

    let chinese_count = CHINESE_RE.find_iter(text).count();
    if chinese_count == 0 {
        return false;
    }

    if chinese_count >= 3 {
        return true;
    }

    let non_whitespace_count = text.chars().filter(|c| !c.is_whitespace()).count();
    if non_whitespace_count == 0 {
        return false;
    }

    (chinese_count as f64 / non_whitespace_count as f64) >= 0.1
}

/// Replace Augment-specific tool names with ace-tool names
pub fn replace_tool_names(text: &str) -> String {
    text.replace("codebase-retrieval", "search_context")
        .replace("codebase_retrieval", "search_context")
}

/// Render the enhance prompt template safely without corrupting user input
/// Uses split+concat instead of replace to avoid replacing placeholders
/// that may appear in user content
pub fn render_enhance_prompt(original_prompt: &str) -> Result<String> {
    let (before, after) = ENHANCE_PROMPT_TEMPLATE
        .split_once("{original_prompt}")
        .ok_or_else(|| anyhow!("ENHANCE_PROMPT_TEMPLATE missing {{original_prompt}}"))?;

    let mut rendered = String::with_capacity(before.len() + original_prompt.len() + after.len());
    rendered.push_str(before);
    rendered.push_str(original_prompt);
    rendered.push_str(after);
    Ok(rendered)
}

/// Build the full prompt for third-party APIs using the template
pub fn build_third_party_prompt(original_prompt: &str) -> Result<String> {
    let enhanced_prompt = render_enhance_prompt(original_prompt)?;

    let language_hint = if is_chinese_text(original_prompt) {
        "\n\n请用中文回复。"
    } else {
        ""
    };

    Ok(format!("{}{}", enhanced_prompt, language_hint))
}

/// Build API URL by joining base URL with a resource path, deduplicating version segments.
///
/// If `base_url` already ends with a version prefix (e.g. `/v1`, `/v1beta`),
/// and `path` also starts with one, the path's version prefix is stripped
/// to avoid duplication like `/v1/v1/messages`.
///
/// Examples:
/// - `("https://api.example.com", "/v1/messages")` → `https://api.example.com/v1/messages`
/// - `("https://api.example.com/v1", "/v1/messages")` → `https://api.example.com/v1/messages`
/// - `("https://proxy.com/v1beta", "/v1/messages")` → `https://proxy.com/v1beta/messages`
pub fn build_api_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if has_version_suffix(base) {
        let path = strip_version_prefix(path);
        format!("{}{}", base, path)
    } else {
        format!("{}{}", base, path)
    }
}

fn has_version_suffix(url: &str) -> bool {
    if let Some(pos) = url.rfind("/v") {
        url[pos + 2..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    } else {
        false
    }
}

fn strip_version_prefix(path: &str) -> &str {
    let p = path.strip_prefix('/').unwrap_or(path);
    if let Some(rest) = p.strip_prefix('v') {
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            // Find end of version segment (next '/' or end of string)
            if let Some(slash) = rest.find('/') {
                return &rest[slash..];
            }
            return "";
        }
    }
    path
}

/// Map common authentication errors to consistent error messages
pub fn map_auth_error(status: u16, provider: &str) -> Option<anyhow::Error> {
    match status {
        401 => Some(anyhow!("{} API key invalid or expired", provider)),
        403 => Some(anyhow!(
            "{} access denied, API key may be disabled",
            provider
        )),
        _ => None,
    }
}

/// Lazy static macro for regex
pub mod lazy_static {
    #[macro_export]
    macro_rules! lazy_static {
        ($(static ref $name:ident: $t:ty = $init:expr;)*) => {
            $(
                static $name: std::sync::LazyLock<$t> = std::sync::LazyLock::new(|| $init);
            )*
        };
    }
    pub use lazy_static;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ====================================================================
    // first_nonempty_text — see ADR-8 in v2 plan §4.4
    // ====================================================================

    #[test]
    fn first_nonempty_text_skips_none_and_empty() {
        let parts = vec![
            None,
            Some(String::new()),
            Some("  ".to_string()),
            Some("real".to_string()),
        ];
        assert_eq!(first_nonempty_text(parts), Some("real".to_string()));
    }

    #[test]
    fn first_nonempty_text_returns_none_when_all_empty() {
        let parts: Vec<Option<String>> = vec![None, Some(String::new()), Some("   ".to_string())];
        assert_eq!(first_nonempty_text(parts), None);
    }

    #[test]
    fn first_nonempty_text_returns_none_for_empty_input() {
        let parts: Vec<Option<String>> = vec![];
        assert_eq!(first_nonempty_text(parts), None);
    }

    #[test]
    fn first_nonempty_text_trims_whitespace() {
        let parts = vec![Some("  hello  ".to_string())];
        assert_eq!(first_nonempty_text(parts), Some("hello".to_string()));
    }

    #[test]
    fn first_nonempty_text_takes_first_non_empty() {
        // First non-empty (after trim) wins, even if later parts are richer
        let parts = vec![
            Some("a".to_string()),
            Some("longer payload".to_string()),
        ];
        assert_eq!(first_nonempty_text(parts), Some("a".to_string()));
    }

    #[test]
    fn first_nonempty_text_handles_gemini_safety_block_pattern() {
        // Realistic Gemini-style: first candidate empty (safety block),
        // second candidate has the real text.
        let parts = vec![
            Some(String::new()),
            Some("real answer".to_string()),
        ];
        assert_eq!(first_nonempty_text(parts), Some("real answer".to_string()));
    }

    // ====================================================================
    // read_nonempty_env / has_nonempty_env — basic sanity coverage
    // ====================================================================

    #[test]
    fn read_nonempty_env_returns_none_for_unset() {
        // Use a key that is extremely unlikely to be set in any environment
        let key = "ACE_TOOL_TEST_DEFINITELY_UNSET_KEY_X9Z7Q";
        std::env::remove_var(key);
        assert_eq!(read_nonempty_env(key), None);
        assert!(!has_nonempty_env(key));
    }

    #[test]
    fn test_build_api_url_no_version_in_base() {
        assert_eq!(
            build_api_url("https://api.example.com", "/v1/messages"),
            "https://api.example.com/v1/messages"
        );
        assert_eq!(
            build_api_url("https://api.example.com/", "/v1/messages"),
            "https://api.example.com/v1/messages"
        );
    }

    #[test]
    fn test_build_api_url_same_version_dedup() {
        assert_eq!(
            build_api_url("https://api.example.com/v1", "/v1/messages"),
            "https://api.example.com/v1/messages"
        );
        assert_eq!(
            build_api_url("https://api.example.com/v1/", "/v1/messages"),
            "https://api.example.com/v1/messages"
        );
        assert_eq!(
            build_api_url("https://api.example.com/v1beta", "/v1beta/models/x:gen"),
            "https://api.example.com/v1beta/models/x:gen"
        );
    }

    #[test]
    fn test_build_api_url_cross_version_dedup() {
        // base has v1beta, path has v1 → keep v1beta, strip v1
        assert_eq!(
            build_api_url("https://proxy.example.com/v1beta", "/v1/messages"),
            "https://proxy.example.com/v1beta/messages"
        );
        // base has v2, path has v1 → keep v2, strip v1
        assert_eq!(
            build_api_url("https://proxy.example.com/v2", "/v1/chat/completions"),
            "https://proxy.example.com/v2/chat/completions"
        );
        // base has v1, path has v1beta → keep v1, strip v1beta
        assert_eq!(
            build_api_url("https://proxy.example.com/v1", "/v1beta/models/x:gen"),
            "https://proxy.example.com/v1/models/x:gen"
        );
    }

    #[test]
    fn test_build_api_url_non_version_path_preserved() {
        assert_eq!(
            build_api_url("https://api.example.com/vertex", "/v1/messages"),
            "https://api.example.com/vertex/v1/messages"
        );
    }
}
