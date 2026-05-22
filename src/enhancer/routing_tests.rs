//! Routing resolution tests using injected env getters.
//!
//! These tests exercise [`crate::enhancer::prompt_enhancer::resolve_endpoint_with`]
//! by supplying a fake `HashMap`-backed env getter, so they do not mutate
//! `std::env` from concurrent test threads (which is the root cause of the
//! historical mutex-poisoning cascade in `tests/prompt_enhancer_test.rs`).
//!
//! See §4.2-4.3 of `docs/prompt_enhancer_redesign_plan_v2.md`.

use std::collections::HashMap;

use crate::enhancer::prompt_enhancer::resolve_endpoint_with;
use crate::service::EnhancerEndpoint;

/// Build a closure that resolves env keys from a captured `HashMap`.
///
/// Whitespace-only / empty values are treated as unset, mirroring the
/// production contract of [`crate::service::read_nonempty_env`].
fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |k| {
        map.get(k)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }
}

// ============================================================================
// Defaults & empty handling
// ============================================================================

#[test]
fn unset_endpoint_defaults_to_local() {
    let env = env_from(&[]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Local);
    assert!(
        d.source.contains("not set"),
        "source should mention not-set, got: {}",
        d.source
    );
}

#[test]
fn empty_endpoint_defaults_to_local() {
    let env = env_from(&[("ACE_ENHANCER_ENDPOINT", "")]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Local);
}

#[test]
fn whitespace_only_endpoint_defaults_to_local() {
    let env = env_from(&[("ACE_ENHANCER_ENDPOINT", "   \t  ")]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Local);
}

// ============================================================================
// Auto-detection (no preferred)
// ============================================================================

#[test]
fn auto_with_no_keys_falls_back_to_local() {
    let env = env_from(&[("ACE_ENHANCER_ENDPOINT", "auto")]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Local);
    assert!(
        d.source.contains("no provider keys"),
        "source should mention no-keys fallback, got: {}",
        d.source
    );
}

#[test]
fn auto_picks_claude_when_only_anthropic_key_set() {
    let env = env_from(&[
        ("ACE_ENHANCER_ENDPOINT", "auto"),
        ("ANTHROPIC_API_KEY", "sk-ant-test"),
    ]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Claude);
    assert!(d.source.contains("ANTHROPIC_API_KEY"));
}

#[test]
fn auto_picks_gemini_when_only_gemini_key_set() {
    let env = env_from(&[
        ("ACE_ENHANCER_ENDPOINT", "auto"),
        ("GEMINI_API_KEY", "AIzaTest"),
    ]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Gemini);
}

#[test]
fn auto_picks_openai_when_only_openai_key_set() {
    let env = env_from(&[
        ("ACE_ENHANCER_ENDPOINT", "auto"),
        ("OPENAI_API_KEY", "sk-test"),
    ]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::OpenAI);
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
    // deterministic order: Claude > Gemini > OpenAI
    assert_eq!(d.endpoint, EnhancerEndpoint::Claude);
}

#[test]
fn auto_prefers_gemini_when_claude_missing() {
    let env = env_from(&[
        ("ACE_ENHANCER_ENDPOINT", "auto"),
        ("OPENAI_API_KEY", "sk-test"),
        ("GEMINI_API_KEY", "AIzaTest"),
    ]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Gemini);
}

#[test]
fn whitespace_only_key_treated_as_unset() {
    let env = env_from(&[
        ("ACE_ENHANCER_ENDPOINT", "auto"),
        ("ANTHROPIC_API_KEY", "   "),
        ("OPENAI_API_KEY", "sk-test"),
    ]);
    let d = resolve_endpoint_with(env).unwrap();
    // Anthropic is whitespace → skip, fall through to OpenAI
    assert_eq!(d.endpoint, EnhancerEndpoint::OpenAI);
}

// ============================================================================
// Preferred provider override
// ============================================================================

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
    assert!(d.source.contains("PREFERRED"));
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
    assert!(result.is_err(), "should fail when preferred key missing");
    let msg = format!("{:?}", result.unwrap_err());
    assert!(
        msg.contains("OPENAI_API_KEY"),
        "error should mention missing key, got: {}",
        msg
    );
}

#[test]
fn preferred_provider_with_unknown_name_errors() {
    let env = env_from(&[
        ("ACE_ENHANCER_ENDPOINT", "auto"),
        ("ACE_ENHANCER_PREFERRED_PROVIDER", "anthropic"), // wrong: should be "claude"
        ("ANTHROPIC_API_KEY", "sk-ant-test"),
    ]);
    let result = resolve_endpoint_with(env);
    assert!(result.is_err());
    let msg = format!("{:?}", result.unwrap_err());
    assert!(msg.contains("PREFERRED_PROVIDER"));
}

#[test]
fn preferred_provider_pointing_to_non_third_party_errors() {
    // `local` is not a third-party provider, so PREFERRED=local should fail
    let env = env_from(&[
        ("ACE_ENHANCER_ENDPOINT", "auto"),
        ("ACE_ENHANCER_PREFERRED_PROVIDER", "local"),
    ]);
    let result = resolve_endpoint_with(env);
    assert!(
        result.is_err(),
        "preferred=local should be rejected (not a third-party provider)"
    );
}

// ============================================================================
// Explicit endpoint values
// ============================================================================

#[test]
fn unknown_endpoint_value_errors() {
    // Wrong name: "anthropic" vs "claude"
    let env = env_from(&[("ACE_ENHANCER_ENDPOINT", "anthropic")]);
    let result = resolve_endpoint_with(env);
    assert!(result.is_err());
    let msg = format!("{:?}", result.unwrap_err());
    assert!(
        msg.contains("Unsupported") || msg.contains("anthropic"),
        "should mention the offending value, got: {}",
        msg
    );
}

#[test]
fn explicit_endpoint_overrides_auto_detect() {
    // Explicit `gemini` wins over the would-be `claude` auto-pick
    let env = env_from(&[
        ("ACE_ENHANCER_ENDPOINT", "gemini"),
        ("ANTHROPIC_API_KEY", "sk-ant-test"),
    ]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Gemini);
}

#[test]
fn local_mode_explicit_does_not_trigger_auto() {
    // Even when ANTHROPIC_API_KEY is present, explicit `local` keeps us offline
    let env = env_from(&[
        ("ACE_ENHANCER_ENDPOINT", "local"),
        ("ANTHROPIC_API_KEY", "sk-ant-test"),
    ]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Local);
}

#[test]
fn explicit_endpoint_is_case_insensitive() {
    let env = env_from(&[("ACE_ENHANCER_ENDPOINT", "CLAUDE")]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Claude);
}

#[test]
fn explicit_endpoint_trims_whitespace() {
    let env = env_from(&[("ACE_ENHANCER_ENDPOINT", "  gemini  ")]);
    let d = resolve_endpoint_with(env).unwrap();
    assert_eq!(d.endpoint, EnhancerEndpoint::Gemini);
}

#[test]
fn all_known_endpoint_values_parse() {
    for (raw, expected) in [
        ("local", EnhancerEndpoint::Local),
        ("new", EnhancerEndpoint::New),
        ("old", EnhancerEndpoint::Old),
        ("claude", EnhancerEndpoint::Claude),
        ("openai", EnhancerEndpoint::OpenAI),
        ("gemini", EnhancerEndpoint::Gemini),
    ] {
        let env = env_from(&[("ACE_ENHANCER_ENDPOINT", raw)]);
        let d = resolve_endpoint_with(env)
            .unwrap_or_else(|e| panic!("'{}' should parse, got error: {:?}", raw, e));
        assert_eq!(d.endpoint, expected, "for raw value '{}'", raw);
    }
}
