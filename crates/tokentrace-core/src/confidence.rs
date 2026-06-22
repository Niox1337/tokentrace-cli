use serde::{Deserialize, Serialize};

/// How trustworthy a recorded value is. Measured and estimated values are
/// always kept separate; mixed totals must be labelled by callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Reported directly by the source.
    Measured,
    /// Derived or approximated, never to be treated as measured.
    Estimated,
    /// Source gave no usable signal.
    Unknown,
}
