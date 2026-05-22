//! Prompt Enhancer - Core enhancement logic
//! Based on Augment VSCode plugin implementation
//!
//! Endpoint selection is controlled by environment variable `ACE_ENHANCER_ENDPOINT`:
//! - `local` or *unset*: Pass through original prompt unchanged (no network call)
//! - `new`: Uses Augment /prompt-enhancer endpoint
//! - `old`: Uses Augment /chat-stream endpoint
//! - `claude` / `openai` / `gemini`: Use respective third-party APIs
//! - `auto`: Auto-detect provider via standard API key env vars
//!   (see [`detect_auto_endpoint`])

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use reqwest::Client;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::service::{
    call_claude_endpoint, call_gemini_endpoint, call_new_endpoint, call_old_endpoint,
    call_openai_endpoint, provider_defaults, read_nonempty_env, resolve_third_party_config,
    EnhancerEndpoint,
};
use crate::utils::project_detector::get_index_file_path;

use super::server::EnhancerServer;

/// Environment variable to control which endpoint to use
pub const ENV_ENHANCER_ENDPOINT: &str = "ACE_ENHANCER_ENDPOINT";

/// Optional user-preferred provider when `ACE_ENHANCER_ENDPOINT=auto`
pub const ENV_ENHANCER_PREFERRED_PROVIDER: &str = "ACE_ENHANCER_PREFERRED_PROVIDER";

/// Result of resolving the active enhancer endpoint, with a human-readable
/// `source` string explaining how the choice was made (for observability).
///
/// See ADR-7 in `docs/prompt_enhancer_redesign_plan_v2.md`.
#[derive(Debug, Clone)]
pub struct EndpointDecision {
    pub endpoint: EnhancerEndpoint,
    pub source: String,
}

/// Resolve the currently active enhancer endpoint based on environment variables.
///
/// Thin wrapper around [`resolve_endpoint_with`] that uses the live process
/// environment via [`read_nonempty_env`]. Tests should call
/// [`resolve_endpoint_with`] directly with an injected env getter to avoid
/// mutating `std::env` from concurrent threads.
///
/// - Unset / empty `ACE_ENHANCER_ENDPOINT` → [`EnhancerEndpoint::Local`] (no network)
/// - `auto` → see [`detect_auto_endpoint_with`]
/// - Any other value → strict parse via [`EnhancerEndpoint::try_from_env_str`]
///   (returns `Err` for unknown values — fail-fast per ADR-6)
pub fn get_enhancer_endpoint() -> Result<EndpointDecision> {
    resolve_endpoint_with(read_nonempty_env)
}

/// Pure-function variant of [`get_enhancer_endpoint`] that takes an injected
/// env getter.
///
/// The getter contract: returns `Some(value)` for non-empty environment
/// values, or `None` for unset/empty/whitespace-only values. This matches
/// the production helper [`read_nonempty_env`].
///
/// Used by the routing tests (`src/enhancer/routing_tests.rs`) to drive
/// the resolution logic with a fake `HashMap`-backed env without touching
/// the real process environment.
///
/// See ADR-7 and §4.2 of `docs/prompt_enhancer_redesign_plan_v2.md`.
pub fn resolve_endpoint_with<F>(env_get: F) -> Result<EndpointDecision>
where
    F: Fn(&str) -> Option<String>,
{
    let raw = env_get(ENV_ENHANCER_ENDPOINT);

    match raw.as_deref().map(str::to_lowercase).as_deref() {
        None => Ok(EndpointDecision {
            endpoint: EnhancerEndpoint::Local,
            source: format!(
                "{} not set, defaulting to Local (no network)",
                ENV_ENHANCER_ENDPOINT
            ),
        }),
        Some("auto") => detect_auto_endpoint_with(&env_get),
        Some(other) => {
            let endpoint = EnhancerEndpoint::try_from_env_str(other)?;
            Ok(EndpointDecision {
                endpoint,
                source: format!("{}={}", ENV_ENHANCER_ENDPOINT, other),
            })
        }
    }
}

/// Auto-detection logic for `ACE_ENHANCER_ENDPOINT=auto`.
///
/// Priority:
/// 1. If `ACE_ENHANCER_PREFERRED_PROVIDER` is set, use it (and require
///    its standard env key to be non-empty, else error).
/// 2. Otherwise scan `ANTHROPIC_API_KEY` > `GEMINI_API_KEY` > `OPENAI_API_KEY`
///    and pick the first non-empty one.
/// 3. If no key is found, fall back to Local with a WARN log
///    (so users get a clear hint instead of silent template fallback).
///
/// Takes `env_get` by reference so that callers can keep ownership of
/// their closure (`resolve_endpoint_with` reuses it across branches).
fn detect_auto_endpoint_with<F>(env_get: &F) -> Result<EndpointDecision>
where
    F: Fn(&str) -> Option<String>,
{
    // Priority 1: explicit preferred provider
    if let Some(preferred) = env_get(ENV_ENHANCER_PREFERRED_PROVIDER) {
        let endpoint = EnhancerEndpoint::try_from_env_str(&preferred)
            .map_err(|e| anyhow!("{}={}: {}", ENV_ENHANCER_PREFERRED_PROVIDER, preferred, e))?;
        let (key_env, _, _) = provider_defaults(endpoint).ok_or_else(|| {
            anyhow!(
                "{}={} but '{}' has no auto-detect support (not a third-party provider)",
                ENV_ENHANCER_PREFERRED_PROVIDER,
                preferred,
                endpoint
            )
        })?;
        if env_get(key_env).is_none() {
            return Err(anyhow!(
                "{}={} specified, but {} is not set or empty",
                ENV_ENHANCER_PREFERRED_PROVIDER,
                preferred,
                key_env
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
        if env_get(key).is_some() {
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
         Falling back to Local (will return original prompt unchanged)."
    );
    Ok(EndpointDecision {
        endpoint: EnhancerEndpoint::Local,
        source: "auto -> no provider keys found, fallback to Local".to_string(),
    })
}

/// Prompt Enhancer
pub struct PromptEnhancer {
    config: Arc<Config>,
    client: Client,
    server: Arc<EnhancerServer>,
}

impl PromptEnhancer {
    /// Create a new PromptEnhancer
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let client = Client::builder().timeout(Duration::from_secs(60)).build()?;

        let server = Arc::new(EnhancerServer::new());

        Ok(Self {
            config,
            client,
            server,
        })
    }

    /// Enhance a prompt with codebase context and conversation history
    ///
    /// # Arguments
    /// * `original_prompt` - The original user input
    /// * `conversation_history` - Conversation history (5-10 rounds)
    /// * `project_root` - Project root path (optional, for loading blob names)
    ///
    /// # Returns
    /// Enhanced prompt text
    pub async fn enhance(
        &self,
        original_prompt: &str,
        conversation_history: &str,
        project_root: Option<&Path>,
    ) -> Result<String> {
        info!("Starting prompt enhancement...");

        // Load blob names if project root is provided
        let blob_names = if let Some(root) = project_root {
            self.load_blob_names(root)
        } else {
            Vec::new()
        };

        if blob_names.is_empty() {
            warn!("No index data found, enhancing without code context");
        } else {
            info!("Loaded {} file chunks", blob_names.len());
        }

        // Set up enhance callback for re-enhancement
        let config = self.config.clone();
        let client = self.client.clone();
        let callback = Arc::new(move |prompt: String, history: String, blobs: Vec<String>| {
            let config = config.clone();
            let client = client.clone();
            Box::pin(async move {
                call_prompt_enhancer_api_static(&client, &config, &prompt, &history, &blobs).await
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send>>
        });
        self.server.set_enhance_callback(callback).await;

        // Call prompt-enhancer API
        info!("Calling prompt-enhancer API...");
        let enhanced_prompt = self
            .call_prompt_enhancer_api(original_prompt, conversation_history, &blob_names)
            .await?;
        info!("Enhancement complete");

        // Start Web UI interaction
        info!("Starting Web UI for user review...");
        let final_prompt = self
            .interact_with_user(
                &enhanced_prompt,
                original_prompt,
                conversation_history,
                &blob_names,
            )
            .await?;

        info!("Prompt enhancement complete");
        Ok(final_prompt)
    }

    /// Interact with user through Web UI
    async fn interact_with_user(
        &self,
        enhanced_prompt: &str,
        original_prompt: &str,
        conversation_history: &str,
        blob_names: &[String],
    ) -> Result<String> {
        // Start server
        self.server.start().await?;

        // Create session (responder is registered at creation time to prevent race conditions)
        let (session_id, rx) = self
            .server
            .create_session(
                enhanced_prompt.to_string(),
                original_prompt.to_string(),
                conversation_history.to_string(),
                blob_names.to_vec(),
            )
            .await;

        // Build URL - get_port() is synchronous in our custom server.rs
        let port = self.server.get_port().ok_or_else(|| anyhow!("Server not started"))?;
        let url = format!("http://localhost:{}/enhance?session={}", port, session_id);
        info!("Please open in browser: {}", url);

        // Try to open browser
        self.open_browser(&url);

        // Wait for user action using the pre-created receiver
        match self
            .server
            .wait_for_session_with_receiver(&session_id, rx)
            .await
        {
            Ok(result) => {
                if result.is_empty() {
                    Err(anyhow!("User cancelled the enhancement"))
                } else {
                    Ok(result)
                }
            }
            Err(e) => {
                if e.to_string().contains("timeout") {
                    error!("User interaction timeout (8 minutes)");
                }
                Err(e)
            }
        }
    }

    /// Open browser
    fn open_browser(&self, url: &str) {
        if let Err(e) = open::that(url) {
            warn!("Could not auto-open browser: {}", e);
            info!("Please manually open: {}", url);
        }
    }

    /// Load blob names from index file
    fn load_blob_names(&self, project_root: &Path) -> Vec<String> {
        let index_file_path = get_index_file_path(project_root);

        if !index_file_path.exists() {
            return Vec::new();
        }

        match std::fs::read_to_string(&index_file_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(names) => names,
                Err(e) => {
                    warn!("Failed to parse index file: {}", e);
                    Vec::new()
                }
            },
            Err(e) => {
                warn!("Failed to read index file: {}", e);
                Vec::new()
            }
        }
    }

    /// Call prompt-enhancer API
    async fn call_prompt_enhancer_api(
        &self,
        original_prompt: &str,
        conversation_history: &str,
        blob_names: &[String],
    ) -> Result<String> {
        call_prompt_enhancer_api_static(
            &self.client,
            &self.config,
            original_prompt,
            conversation_history,
            blob_names,
        )
        .await
    }

    /// Simple enhancement without Web UI interaction
    /// Used for CLI mode where we just want the enhanced prompt output
    pub async fn enhance_simple(
        &self,
        original_prompt: &str,
        conversation_history: &str,
        project_root: Option<&Path>,
    ) -> Result<String> {
        info!("Starting simple prompt enhancement (no Web UI)...");

        // Load blob names if project root is provided
        let blob_names = if let Some(root) = project_root {
            self.load_blob_names(root)
        } else {
            Vec::new()
        };

        if blob_names.is_empty() {
            warn!("No index data found, enhancing without code context");
        } else {
            info!("Loaded {} file chunks", blob_names.len());
        }

        // Call prompt-enhancer API directly
        info!("Calling prompt-enhancer API...");
        let enhanced_prompt = self
            .call_prompt_enhancer_api(original_prompt, conversation_history, &blob_names)
            .await?;

        info!("Enhancement complete");
        Ok(enhanced_prompt)
    }
}

/// Static function to call prompt-enhancer API (used for callback)
async fn call_prompt_enhancer_api_static(
    client: &Client,
    config: &Config,
    original_prompt: &str,
    conversation_history: &str,
    blob_names: &[String],
) -> Result<String> {
    let decision = get_enhancer_endpoint()?;
    info!(
        endpoint = %decision.endpoint,
        source = %decision.source,
        "Prompt enhancer endpoint resolved"
    );

    match decision.endpoint {
        EnhancerEndpoint::Local => {
            // ADR-2: Local mode no longer renders the meta-template.
            // The template (with its "⚠️ NO TOOLS ALLOWED ⚠️" banner) is
            // an instruction-for-an-LLM, not a result-for-the-MCP-client.
            // Returning the original prompt unchanged is the correct
            // graceful-degradation behavior.
            warn!(
                "Using LOCAL mode: returning original prompt unchanged. \
                 To enable enhancement, set ACE_ENHANCER_ENDPOINT=auto \
                 (with API key inheritance) or ACE_ENHANCER_ENDPOINT=claude/gemini/openai."
            );
            Ok(original_prompt.to_string())
        }
        EnhancerEndpoint::New => {
            info!("Using NEW prompt-enhancer endpoint (Augment)");
            call_new_endpoint(client, config, original_prompt, conversation_history).await
        }
        EnhancerEndpoint::Old => {
            info!("Using OLD chat-stream endpoint (Augment)");
            call_old_endpoint(
                client,
                config,
                original_prompt,
                conversation_history,
                blob_names,
            )
            .await
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
                EnhancerEndpoint::Claude => {
                    call_claude_endpoint(
                        client,
                        &resolved.config,
                        original_prompt,
                        conversation_history,
                    )
                    .await
                }
                EnhancerEndpoint::OpenAI => {
                    call_openai_endpoint(
                        client,
                        &resolved.config,
                        original_prompt,
                        conversation_history,
                    )
                    .await
                }
                EnhancerEndpoint::Gemini => {
                    call_gemini_endpoint(
                        client,
                        &resolved.config,
                        original_prompt,
                        conversation_history,
                    )
                    .await
                }
                _ => unreachable!("guarded by outer match arm"),
            }
        }
    }
}
