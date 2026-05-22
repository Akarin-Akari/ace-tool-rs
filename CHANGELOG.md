# Changelog

All notable changes to this fork of `ace-tool-rs` are documented here.
The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

- **Prompt enhancer v2 architecture** — Landed 9 ADRs covering opt-in
  auto-detection, strict provider boundaries, multi-key precedence, and
  graceful degradation. See `docs/prompt_enhancer_redesign_plan_v2.md`
  for the full blueprint.
- `ACE_ENHANCER_ENDPOINT=auto` — Opt-in auto-detection mode that scans
  `ANTHROPIC_API_KEY` → `GEMINI_API_KEY` → `OPENAI_API_KEY` and selects
  the first non-empty key. Falls back to `local` with a `WARN` log if
  no keys are present.
- `ACE_ENHANCER_PREFERRED_PROVIDER` — Optional override for `auto` mode
  to force a specific provider (`claude` / `openai` / `gemini`) instead
  of the deterministic scan order.
- `EnhancerEndpoint::try_from_env_str()` — Strict, fallible parser that
  returns `Err` for unknown values (fail-fast per ADR-1). The legacy
  `from_env_str()` is now `#[deprecated]` and silently falls back to
  `Local` for backward compatibility.
- `EndpointDecision { endpoint, source }` — Carries the resolved endpoint
  along with a human-readable `source` string explaining how the choice
  was made, for observability.
- `ResolvedThirdPartyConfig` — Surfaces `token_source` / `base_url_source`
  / `model_source` provenance metadata without leaking secret values.
- `first_nonempty_text()` helper — Hardens Gemini/OpenAI response parsing
  against empty-string parts and safety-block scenarios.
- `resolve_endpoint_with<F: Fn(&str) -> Option<String>>()` — Pure-function
  variant of `get_enhancer_endpoint` that takes an injected env getter,
  enabling thread-safe routing tests via fake `HashMap`-backed env.
- Test suite `src/enhancer/routing_tests.rs` — 19 injection-style
  resolution tests covering defaults, auto-detection priority, preferred
  provider override, explicit endpoints, and edge cases.
- README — New "Prompt Enhancer Endpoint Selection" section documenting
  all six endpoint modes plus auto-detection rules and a migration guide.

### Changed

- **Default `ACE_ENHANCER_ENDPOINT` is now `local`** (was `new`).
  `local` returns the original prompt unchanged with no network call,
  giving safer and more predictable defaults. Users who relied on the
  implicit `new` default must now set `ACE_ENHANCER_ENDPOINT=new`
  explicitly.
- **`PROMPT_ENHANCER_BASE_URL` / `PROMPT_ENHANCER_MODEL` are now
  optional** — Each falls back to per-provider sensible defaults
  (e.g. `https://api.anthropic.com` for Claude) when unset, instead
  of failing the request (ADR-5).
- **`PROMPT_ENHANCER_TOKEN` precedence** — When unset, the resolver now
  falls back to the provider-standard env var (`ANTHROPIC_API_KEY`,
  `GEMINI_API_KEY`, `OPENAI_API_KEY`) instead of erroring out (ADR-5).
- **Gemini / OpenAI response parsing** — Replaced the fragile
  `.first().and_then(...)` chain with `first_nonempty_text()`, which
  correctly handles empty first candidates (Gemini safety-block pattern)
  and whitespace-only responses (ADR-8).
- **Structured logging on every enhance call** — Emits `endpoint`,
  `source`, `token_source`, `base_url_source`, and `model_source` at
  `info!` level. Secret values are never logged (ADR-7).
- **`enhance_prompt` tool description** — Added an `IMPORTANT AFTER THE
  TOOL RETURNS` clause instructing agents to treat the returned text as
  a refined version of the user's instructions and continue fulfilling
  the original request, rather than stopping after displaying the
  enhanced prompt. (Cherry-picked from upstream `missdeer/ace-tool-rs`
  commit `2658492`.)

### Fixed

- **HTTP 429 from Augment** — Bypasses Augment's UA risk-control by
  using a more realistic browser User-Agent string when calling the
  Augment cloud endpoints (`new` and `old`).
- **`EnhancerEndpoint::Local` no longer leaks the meta-template** —
  Previously, `Local` mode rendered the internal `⚠️ NO TOOLS ALLOWED ⚠️`
  meta-template and returned it as the "enhanced result", causing
  severe UX confusion. It now short-circuits and returns the original
  prompt unchanged (ADR-2).
- **`get_third_party_config` no longer panics on missing optional
  config** — Previously failed hard on missing `PROMPT_ENHANCER_BASE_URL`
  even when the endpoint had a sensible default. Now degrades
  gracefully (ADR-5).

### Deprecated

- `EnhancerEndpoint::from_env_str()` — Use `try_from_env_str()` instead
  for fail-fast semantics. The deprecated variant silently returns
  `Local` for unknown values, which masks configuration typos.

### Internal

- `src/enhancer/server.rs` — Visibility adjustments (`pub fn cors_response`,
  `pub fn json_response`, `pub fn serve_enhancer_ui`, `pub timeout_ms`) so
  integration tests in the separate test crate can compile.
- Test reorganization — Rewrote 3 endpoint-resolution tests for the new
  default and fail-fast semantics. Added 4 new tests for
  `try_from_env_str` and legacy `from_env_str` regression. Reworked
  `get_third_party_config` tests around ADR-5 fallback behaviour.

---

## [0.1.10] — 2026-05-22

### Added

- Dual license (MIT + Apache-2.0).

### Fixed

- Fail-closed strategy when `strip_prefix` fails in exclude checks
  (prevents accidentally indexing files that should have been skipped).

### Changed

- Custom `OnceCell`-based `EnhancerServer` with port 18080 (later
  reverted to port 3000 for better compatibility).

---

[Unreleased]: https://github.com/Akarin-Akari/ace-tool-rs/compare/v0.1.10...HEAD
[0.1.10]: https://github.com/Akarin-Akari/ace-tool-rs/releases/tag/v0.1.10
