//! Diagnostics deliberately contain no YAML source, values, or dependency errors.

use std::fmt;

/// Stable categories safe to return through API errors and structured logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ErrorKind {
    /// The scanner or event stream rejected the document.
    #[error("invalid YAML syntax")]
    Syntax,
    /// Backend grammars disagree on compact question-mark flow entries.
    #[error(
        "ambiguous YAML flow '?' syntax; separate the key indicator with a space or quote the scalar"
    )]
    AmbiguousFlowIndicator,
    /// YAML backends disagree on whether these literal characters end a line.
    #[error(
        "literal U+0085/U+2028/U+2029 is unsupported in YAML; use LF for line breaks or a Unicode escape in a double-quoted scalar"
    )]
    AmbiguousLineBreak,
    /// Exactly one YAML document is required.
    #[error("exactly one YAML document is required")]
    Document,
    /// Input bytes exceed the selected profile.
    #[error("YAML input exceeds the byte limit")]
    Input,
    /// Alias-expanded values exceed the selected profile.
    #[error("expanded YAML exceeds the value budget")]
    Budget,
    /// Logical nesting exceeds the selected profile.
    #[error("YAML nesting exceeds the depth limit")]
    Depth,
    /// An unresolved, recursive, or excessively repeated alias was found.
    #[error("invalid or excessive YAML alias expansion")]
    Alias,
    /// A custom tag or a tag on an incompatible node was supplied.
    #[error("unsupported YAML tag")]
    Tag,
    /// Ordered template mappings reject duplicate keys.
    #[error("duplicate YAML mapping key")]
    DuplicateKey,
    /// A merge entry must contain mappings.
    #[error("YAML merge requires a mapping or list of mappings")]
    Merge,
    /// The value does not fit the requested typed representation.
    #[error("YAML value has an invalid type or range")]
    Type,
    /// YAML serialization failed.
    #[error("YAML serialization failed")]
    Serialize,
}

/// A source-free diagnostic with an optional one-based line and column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    location: Option<(usize, usize)>,
}

impl Error {
    /// Construct an unlocated diagnostic without carrying source data.
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            location: None,
        }
    }

    /// Return the stable error category.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Return one-based line and column, when the parser supplied them.
    pub fn location(&self) -> Option<(usize, usize)> {
        self.location
    }

    pub(crate) fn at(mut self, mark: granit_parser::Marker) -> Self {
        self.location = Some((mark.line(), mark.col().saturating_add(1)));
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)?;
        if let Some((line, column)) = self.location {
            write!(f, " at line {line}, column {column}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

impl serde::de::Error for Error {
    fn custom<T: fmt::Display>(_: T) -> Self {
        // Typed visitors may embed passwords, URLs or arbitrary map keys.
        Self::new(ErrorKind::Type)
    }
}
