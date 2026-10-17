//! Source errors as plain data, so that a module's compile error keeps its
//! code, range, and fixes as it crosses the loader and VM host boundaries.
//!
//! The frontend owns how these errors are found; this crate only carries them.

use alloc::{boxed::Box, string::String, vec::Vec};
use core::fmt;

use crate::SourceSpan;

/// The compiler stage that rejected the source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceErrorKind {
    Parse,
    Semantic,
}

/// A parse or semantic error in one module's source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDiagnostic {
    pub kind: SourceErrorKind,
    /// The stable failure class (`docs/reference/diagnostic-codes.md`).
    pub code: &'static str,
    pub message: String,
    pub span: Option<SourceSpan>,
    /// The extent of the offending source in `span`'s file, when known.
    pub range: Option<SourceRange>,
    /// What was required here, such as a type or token, when known.
    pub expected: Option<Box<str>>,
    /// What was found instead, when known.
    pub actual: Option<Box<str>>,
    /// Machine-applicable fixes, each a set of edits in `span`'s file.
    pub fixes: Box<[SourceFix]>,
}

impl fmt::Display for SourceDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(span) = &self.span {
            write!(f, " at {}:{}:{}", span.path, span.line, span.column)?;
        }
        Ok(())
    }
}

/// One machine-applicable fix: apply all of its edits together.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFix {
    pub message: String,
    pub edits: Vec<SourceEdit>,
}

impl SourceFix {
    /// Applies this fix to `source`, the text its error was reported against.
    /// Returns `None` when an edit lies outside `source` or edits overlap.
    #[must_use]
    pub fn apply(&self, source: &str) -> Option<String> {
        apply_fix(source, self)
    }
}

/// Applies `fix` to `source`, the text its error was reported against.
/// Returns `None` when an edit lies outside `source` or edits overlap.
#[must_use]
pub fn apply_fix(source: &str, fix: &SourceFix) -> Option<String> {
    let mut edits = fix
        .edits
        .iter()
        .map(|edit| {
            let start = char_offset(source, edit.range.start)?;
            let end = char_offset(source, edit.range.end)?;
            (start <= end).then_some((start, end, edit.text.as_str()))
        })
        .collect::<Option<Vec<_>>>()?;
    edits.sort_by_key(|(start, _, _)| *start);
    if edits.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return None;
    }
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for (start, end, text) in edits {
        output.push_str(&source[cursor..start]);
        output.push_str(text);
        cursor = end;
    }
    output.push_str(&source[cursor..]);
    Some(output)
}

/// The byte offset of a 1-based line and scalar-value column, allowing the
/// position just past the end of a line or of the text.
fn char_offset(source: &str, position: SourcePosition) -> Option<usize> {
    let mut line = 1;
    let mut column = 1;
    for (offset, character) in source.char_indices() {
        if (line, column) == (position.line, position.column) {
            return Some(offset);
        }
        if character == '\n' {
            if line == position.line {
                return None;
            }
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    ((line, column) == (position.line, position.column)).then_some(source.len())
}

/// Replace `range` with `text`. An empty range inserts; empty text deletes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceEdit {
    pub range: SourceRange,
    pub text: String,
}

/// A source extent from `start` up to `end`, exclusive.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

/// A 1-based line and column; columns count Unicode scalar values.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourcePosition {
    pub line: u32,
    pub column: u32,
}
