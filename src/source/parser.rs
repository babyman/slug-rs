use super::ast::Token;

/// Stateful source parser. Grammar methods remain with the front-end during
/// the staged extraction so parsing behavior is unchanged.
pub(super) struct Parser {
    pub(super) tokens: Vec<Token>,
    pub(super) index: usize,
    pub(super) nesting: usize,
}

impl Parser {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0, nesting: 0 }
    }
}
