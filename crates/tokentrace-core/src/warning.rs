use serde::{Deserialize, Serialize};

/// Category of a non-fatal problem found while importing or correlating data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    /// A record could not be linked to its session, turn, or request.
    MissingCorrelationKey,
    /// The source carried a field TokenTrace does not yet model.
    UnsupportedField,
    /// Content was withheld or redacted by a privacy gate.
    Redaction,
    /// The source shape drifted from what the adapter expects.
    SchemaDrift,
    /// A value is estimated and must not be read as measured.
    EstimateCaveat,
}

/// A non-fatal problem surfaced to the user, with human-readable context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    pub message: String,
    /// Optional source or record identifier the warning relates to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl Warning {
    pub fn new(kind: WarningKind, message: impl Into<String>) -> Self {
        Warning {
            kind,
            message: message.into(),
            context: None,
        }
    }
}
