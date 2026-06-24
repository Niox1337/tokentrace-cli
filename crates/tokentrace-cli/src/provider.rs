//! Provider identity and brand colours.
//!
//! A small registry that maps a provider string to a stable brand colour for
//! the usage bar. Keys are normalised and aliased first (`claude` resolves to
//! `anthropic`, `gpt` to `openai`, and so on), the well-known vendors carry a
//! seeded colour, and anything else gets a deterministic colour from a fixed
//! fallback palette. Pure and terminal-free so it can be unit-tested.

use ratatui::style::Color;
use sha2::{Digest, Sha256};

/// Normalise a provider string to a canonical vendor key: trimmed, lowercased,
/// and aliased. Unknown keys are returned as-is (still trimmed and lowercased)
/// so they stay stable across calls.
pub fn canonical(provider: &str) -> String {
    let key = provider.trim().to_ascii_lowercase();
    match key.as_str() {
        "anthropic" | "claude" => "anthropic",
        "openai" | "gpt" | "chatgpt" => "openai",
        "google" | "gemini" | "palm" | "bard" => "google",
        "meta" | "llama" => "meta",
        "mistral" | "mixtral" => "mistral",
        "cohere" | "command" => "cohere",
        "deepseek" => "deepseek",
        "xai" | "grok" => "xai",
        "amazon" | "nova" | "bedrock" | "aws" => "amazon",
        "microsoft" | "azure" | "phi" => "microsoft",
        other => other,
    }
    .to_string()
}

/// The brand colour for a provider. Well-known vendors get their seeded colour;
/// anything else gets a deterministic colour from the fallback palette, keyed by
/// a hash of the canonical name. Never panics, even on an empty provider.
pub fn provider_color(provider: &str) -> Color {
    match canonical(provider).as_str() {
        "anthropic" => Color::Rgb(217, 119, 87),
        "openai" => Color::Rgb(16, 163, 127),
        "google" => Color::Rgb(66, 133, 244),
        "meta" => Color::Rgb(8, 102, 255),
        "mistral" => Color::Rgb(250, 82, 15),
        "cohere" => Color::Rgb(209, 142, 226),
        "deepseek" => Color::Rgb(77, 107, 254),
        // Seeded near-black for xAI, nudged to a visible slate so the segment
        // does not vanish on a dark terminal. The legend names it regardless.
        "xai" => Color::Rgb(120, 120, 128),
        "amazon" => Color::Rgb(255, 153, 0),
        "microsoft" => Color::Rgb(0, 120, 212),
        other => fallback_color(other),
    }
}

/// A fixed palette of distinct hues for providers with no seeded brand colour.
/// Chosen to stay apart from each other and from the seeded brand colours.
const FALLBACK: [Color; 8] = [
    Color::Rgb(220, 80, 100),  // red-pink
    Color::Rgb(90, 200, 160),  // mint
    Color::Rgb(180, 140, 40),  // gold
    Color::Rgb(150, 110, 220), // violet
    Color::Rgb(70, 180, 200),  // cyan
    Color::Rgb(230, 130, 60),  // amber
    Color::Rgb(120, 170, 90),  // green
    Color::Rgb(200, 100, 180), // magenta
];

/// Deterministically pick a fallback colour for an unseeded key.
fn fallback_color(key: &str) -> Color {
    let digest = Sha256::digest(key.as_bytes());
    FALLBACK[digest[0] as usize % FALLBACK.len()]
}

/// Best-effort provider for a request that carries no provider string, derived
/// from the model name prefix. Returns a canonical vendor key, or `unknown`.
pub fn provider_from_model(model: &str) -> &'static str {
    let m = model.trim().to_ascii_lowercase();
    let prefix = |p: &str| m.starts_with(p);
    if prefix("claude") {
        "anthropic"
    } else if prefix("gpt") || prefix("chatgpt") || prefix("o1") || prefix("o3") || prefix("o4") {
        "openai"
    } else if prefix("gemini") || prefix("palm") {
        "google"
    } else if prefix("llama") || prefix("meta") {
        "meta"
    } else if prefix("mistral") || prefix("mixtral") {
        "mistral"
    } else if prefix("command") {
        "cohere"
    } else if prefix("deepseek") {
        "deepseek"
    } else if prefix("grok") {
        "xai"
    } else if prefix("nova") {
        "amazon"
    } else if prefix("phi") {
        "microsoft"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_resolve_to_one_brand_colour() {
        // claude and anthropic are the same vendor, so the same colour.
        assert_eq!(provider_color("claude"), provider_color("anthropic"));
        assert_eq!(provider_color("anthropic"), Color::Rgb(217, 119, 87));
        // Normalisation handles case and surrounding whitespace.
        assert_eq!(provider_color("  GPT "), provider_color("openai"));
        assert_eq!(canonical("  Claude  "), "anthropic");
    }

    #[test]
    fn unknown_provider_is_deterministic_and_stable() {
        // An invented name draws a stable colour from the fallback palette.
        let first = provider_color("acme-llm");
        assert_eq!(first, provider_color("acme-llm"));
        assert!(FALLBACK.contains(&first));
    }

    #[test]
    fn empty_or_blank_provider_never_panics() {
        let _ = provider_color("");
        let _ = provider_color("   ");
        assert!(FALLBACK.contains(&provider_color("")));
    }

    #[test]
    fn provider_from_model_maps_known_prefixes() {
        assert_eq!(provider_from_model("claude-opus-4-8"), "anthropic");
        assert_eq!(provider_from_model("gpt-4o-mini"), "openai");
        assert_eq!(provider_from_model("o3-mini"), "openai");
        assert_eq!(provider_from_model("gemini-1.5-pro"), "google");
        assert_eq!(provider_from_model("llama-3.1-70b"), "meta");
        assert_eq!(provider_from_model("mistral-large"), "mistral");
        assert_eq!(provider_from_model("command-r-plus"), "cohere");
        assert_eq!(provider_from_model("deepseek-chat"), "deepseek");
        assert_eq!(provider_from_model("grok-2"), "xai");
        assert_eq!(provider_from_model("nova-pro"), "amazon");
        assert_eq!(provider_from_model("phi-3"), "microsoft");
        assert_eq!(provider_from_model("some-local-model"), "unknown");
    }
}
