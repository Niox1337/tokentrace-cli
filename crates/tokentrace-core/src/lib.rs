//! TokenTrace owned model: confidence, warnings, and the core domain types.
//!
//! Every source maps into these types. Measured and estimated values are kept
//! separate via [`Confidence`]; callers must label any mixed total.

mod confidence;
mod model;
mod warning;

pub use confidence::Confidence;
pub use model::{
    CostUsage, DiffSummary, FileEvent, GitCommit, Millis, ModelRequest, Session, SessionStatus,
    Timestamp, TokenUsage, ToolCall, Turn,
};
pub use warning::{Warning, WarningKind};

// TODO(0.3.0): add AgentSource and its capability report alongside the adapter trait.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_serializes_lowercase() {
        let json = serde_json::to_string(&Confidence::Estimated).unwrap();
        assert_eq!(json, "\"estimated\"");
        let back: Confidence = serde_json::from_str("\"measured\"").unwrap();
        assert_eq!(back, Confidence::Measured);
    }

    #[test]
    fn warning_roundtrips_with_kind() {
        let w = Warning::new(WarningKind::EstimateCaveat, "token count is estimated");
        let json = serde_json::to_string(&w).unwrap();
        let back: Warning = serde_json::from_str(&json).unwrap();
        assert_eq!(w, back);
        assert!(json.contains("estimate_caveat"));
    }
}
