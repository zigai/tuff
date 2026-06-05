use ruff_text_size::TextRange;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomLayoutDiagnostic {
    pub kind: CustomLayoutDiagnosticKind,
    pub range: Option<TextRange>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CustomLayoutDiagnosticKind {
    UnsupportedInput,
    InternalInvariant,
}

#[expect(
    dead_code,
    reason = "kept as the typed error surface for custom layout expansion"
)]
#[derive(Debug, thiserror::Error)]
pub(crate) enum CustomLayoutError {
    #[error("{message}")]
    Internal { message: &'static str },
}
