/// Stateful source scanner. Tokenization methods remain in the parent module
/// during this first extraction step so lexer behavior stays unchanged.
pub(super) struct Lexer {
    pub(super) path: String,
    pub(super) input: Vec<char>,
    pub(super) index: usize,
    pub(super) line: u32,
    pub(super) column: u32,
}
