// frontend/lexer.rs
//
// IEC 61131-3 Structured Text Lexer
// Covers keywords, data types, operators, literals, and comments
// per IEC 61131-3:2025 (4th edition, IL removed)

// ─── Token Types ────────────────────────────────────────────────

/// Classification of an IEC 61131-3 Structured Text token.
///
/// Each variant corresponds to a lexical element defined by the
/// IEC 61131-3:2025 standard. The lexer maps raw source text to
/// one of these variants, which the parser then consumes to build
/// an Abstract Syntax Tree.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TokenType {
    // ── Special ──

    /// End of input — the final token emitted by the lexer.
    Eof,
    /// An unrecognised character or sequence that does not match
    /// any IEC 61131-3 lexical rule.
    Unknown,

    // ── Identifiers & Literals ──

    /// A user-defined identifier such as a variable, function, or
    /// program name (e.g. `MyCounter`, `speed_ref`).
    Ident,
    /// An integer literal, including decimal (`42`), based
    /// (`16#FF`, `8#77`, `2#1010`), and underscore-separated
    /// (`1_000`) forms.
    IntLiteral,
    /// A real (floating-point) literal with optional exponent
    /// (e.g. `3.14`, `1.0e-3`, `0.5E+2`).
    RealLiteral,
    /// A single-byte string literal delimited by single quotes
    /// (e.g. `'hello'`). Uses `$` as the IEC 61131-3 escape character.
    StringLiteral,
    /// A wide (double-byte) string literal delimited by double
    /// quotes (e.g. `"hello"`).
    WStringLiteral,
    /// A duration literal prefixed with `T#` or `TIME#`
    /// (e.g. `T#5s`, `TIME#1h2m3s`).
    TimeLiteral,
    /// A date literal prefixed with `D#` or `DATE#`
    /// (e.g. `D#2025-12-01`).
    DateLiteral,
    /// A time-of-day literal prefixed with `TOD#` or `TIME_OF_DAY#`
    /// (e.g. `TOD#14:30:00`).
    TodLiteral,
    /// A combined date-and-time literal prefixed with `DT#` or
    /// `DATE_AND_TIME#` (e.g. `DT#2025-12-01-14:30:00`).
    DtLiteral,
    /// A boolean literal: `TRUE` or `FALSE`.
    BoolLiteral,

    // ── Program Organisation Units ──

    /// `PROGRAM` keyword — begins a program declaration.
    Program,
    /// `END_PROGRAM` keyword — terminates a program declaration.
    EndProgram,
    /// `FUNCTION` keyword — begins a function declaration.
    Function,
    /// `END_FUNCTION` keyword — terminates a function declaration.
    EndFunction,
    /// `FUNCTION_BLOCK` keyword — begins a function block declaration.
    FunctionBlock,
    /// `END_FUNCTION_BLOCK` keyword — terminates a function block declaration.
    EndFunctionBlock,

    // ── Variable Declarations ──

    /// `VAR` keyword — begins a local variable block.
    Var,
    /// `END_VAR` keyword — terminates any variable block.
    EndVar,
    /// `VAR_INPUT` keyword — begins an input variable block.
    VarInput,
    /// `VAR_OUTPUT` keyword — begins an output variable block.
    VarOutput,
    /// `VAR_IN_OUT` keyword — begins an in-out variable block.
    VarInOut,
    /// `VAR_GLOBAL` keyword — begins a global variable block.
    VarGlobal,
    /// `VAR_EXTERNAL` keyword — begins an external variable block.
    VarExternal,
    /// `VAR_TEMP` keyword — begins a temporary variable block.
    VarTemp,
    /// `RETAIN` qualifier — marks variables as retentive.
    Retain,
    /// `CONSTANT` qualifier — marks variables as constant.
    Constant,
    /// `AT` keyword — used for direct address binding.
    At,

    // ── Data Type Keywords ──

    /// `BOOL` — 1-bit boolean type.
    TypeBool,
    /// `SINT` — 8-bit signed integer.
    TypeSint,
    /// `INT` — 16-bit signed integer.
    TypeInt,
    /// `DINT` — 32-bit signed integer.
    TypeDint,
    /// `LINT` — 64-bit signed integer.
    TypeLint,
    /// `USINT` — 8-bit unsigned integer.
    TypeUsint,
    /// `UINT` — 16-bit unsigned integer.
    TypeUint,
    /// `UDINT` — 32-bit unsigned integer.
    TypeUdint,
    /// `ULINT` — 64-bit unsigned integer.
    TypeUlint,
    /// `REAL` — 32-bit IEEE 754 floating-point.
    TypeReal,
    /// `LREAL` — 64-bit IEEE 754 floating-point.
    TypeLreal,
    /// `BYTE` — 8-bit bit string.
    TypeByte,
    /// `WORD` — 16-bit bit string.
    TypeWord,
    /// `DWORD` — 32-bit bit string.
    TypeDword,
    /// `LWORD` — 64-bit bit string.
    TypeLword,
    /// `STRING` — single-byte character string.
    TypeString,
    /// `WSTRING` — wide (double-byte) character string.
    TypeWstring,
    /// `TIME` — duration type.
    TypeTime,
    /// `DATE` — calendar date type.
    TypeDate,
    /// `TIME_OF_DAY` / `TOD` — time-of-day type.
    TypeTod,
    /// `DATE_AND_TIME` / `DT` — combined date-and-time type.
    TypeDt,

    // ── User-Defined Types ──

    /// `TYPE` keyword — begins a type declaration.
    Type,
    /// `END_TYPE` keyword — terminates a type declaration.
    EndType,
    /// `STRUCT` keyword — begins a structure definition.
    Struct,
    /// `END_STRUCT` keyword — terminates a structure definition.
    EndStruct,
    /// `ARRAY` keyword — used in array type declarations.
    Array,
    /// `OF` keyword — separates array bounds from element type,
    /// or case selector from case body.
    Of,

    // ── Control Flow ──

    /// `IF` keyword — begins a conditional statement.
    If,
    /// `THEN` keyword — separates condition from body.
    Then,
    /// `ELSIF` keyword — alternative condition branch.
    Elsif,
    /// `ELSE` keyword — default branch.
    Else,
    /// `END_IF` keyword — terminates an IF statement.
    EndIf,
    /// `CASE` keyword — begins a case/switch statement.
    Case,
    /// `END_CASE` keyword — terminates a CASE statement.
    EndCase,
    /// `FOR` keyword — begins a counted loop.
    For,
    /// `TO` keyword — upper bound in a FOR loop.
    To,
    /// `BY` keyword — step increment in a FOR loop.
    By,
    /// `DO` keyword — separates loop header from body.
    Do,
    /// `END_FOR` keyword — terminates a FOR loop.
    EndFor,
    /// `WHILE` keyword — begins a pre-condition loop.
    While,
    /// `END_WHILE` keyword — terminates a WHILE loop.
    EndWhile,
    /// `REPEAT` keyword — begins a post-condition loop.
    Repeat,
    /// `UNTIL` keyword — post-condition in a REPEAT loop.
    Until,
    /// `END_REPEAT` keyword — terminates a REPEAT loop.
    EndRepeat,
    /// `EXIT` keyword — breaks out of the innermost loop.
    Exit,
    /// `RETURN` keyword — early return from a function or function block.
    Return,

    // ── Boolean / Bitwise Operators (keyword form) ──

    /// `AND` keyword — logical/bitwise AND. Also matched by `&`.
    And,
    /// `OR` keyword — logical/bitwise OR.
    Or,
    /// `XOR` keyword — logical/bitwise exclusive OR.
    Xor,
    /// `NOT` keyword — logical/bitwise negation.
    Not,
    /// `MOD` keyword — integer modulo operator.
    Mod,

    // ── Arithmetic Operators ──

    /// `+` — addition or unary plus.
    Plus,
    /// `-` — subtraction or unary minus.
    Minus,
    /// `*` — multiplication.
    Star,
    /// `/` — division.
    Slash,
    /// `**` — exponentiation.
    Power,

    // ── Comparison Operators ──

    /// `=` — equality comparison.
    Equal,
    /// `<>` — inequality comparison.
    NotEqual,
    /// `<` — less-than comparison.
    Less,
    /// `<=` — less-than-or-equal comparison.
    LessEq,
    /// `>` — greater-than comparison.
    Greater,
    /// `>=` — greater-than-or-equal comparison.
    GreaterEq,

    // ── Assignment & Connectors ──

    /// `:=` — variable assignment.
    Assignment,
    /// `=>` — output assignment in function block calls.
    OutputAssign,
    /// `->` — SFC transition connector.
    Arrow,

    // ── Delimiters ──

    /// `:` — type separator in declarations.
    Colon,
    /// `;` — statement terminator.
    SemiColon,
    /// `,` — separator in lists.
    Comma,
    /// `.` — member access (e.g. `fb_instance.output`).
    Dot,
    /// `..` — range operator (e.g. `0..9` in ARRAY bounds, CASE labels).
    DotDot,
    /// `(` — opening parenthesis.
    LParen,
    /// `)` — closing parenthesis.
    RParen,
    /// `[` — opening bracket (array subscript).
    LBracket,
    /// `]` — closing bracket (array subscript).
    RBracket,
    /// `#` — typed literal prefix separator (e.g. `INT#5`).
    Hash,
    /// `&` — alternate symbol for the AND operator.
    Ampersand,
}

/// A single token produced by the lexer.
///
/// Each token carries its [`TokenType`] classification, the exact
/// source text that was matched, and the line/column where it begins
/// (both 1-indexed).
#[derive(Debug, Clone)]
pub struct Token {
    /// The classification of this token.
    pub kind: TokenType,
    /// The exact text extracted from the source code.
    pub text: String,
    /// The 1-indexed line number where the token begins.
    pub line: usize,
    /// The 1-indexed column number where the token begins.
    pub col: usize,
}

// ─── Lexer ──────────────────────────────────────────────────────

/// A lexer (scanner) for IEC 61131-3 Structured Text.
///
/// Converts a raw source string into a sequence of [`Token`]s.
/// The lexer handles:
///
/// - All IEC 61131-3:2025 keywords
/// - Integer, real, boolean, string, and temporal literals
/// - Block comments `(* ... *)` with nesting support
/// - Line comments `// ...`
/// - Multi-character operators (`:=`, `<=`, `<>`, `**`, `..`, etc.)
/// - Source location tracking (line and column)
///
/// # Example
///
/// ```
/// use sdplc::lexer::{Lexer, TokenType};
///
/// let mut lexer = Lexer::new("VAR x : INT := 42; END_VAR");
/// let tokens = lexer.tokenize();
///
/// assert_eq!(tokens[0].kind, TokenType::Var);
/// assert_eq!(tokens[5].kind, TokenType::IntLiteral);
/// assert_eq!(tokens[5].text, "42");
/// ```
pub struct Lexer {
    /// The source code as a vector of characters for indexed access.
    input: Vec<char>,
    /// Current read position in the input vector.
    position: usize,
    /// The character at `position`, or `None` at end-of-input.
    current_char: Option<char>,
    /// Current line number (1-indexed).
    line: usize,
    /// Current column number (1-indexed).
    col: usize,
}

impl Lexer {
    /// Creates a new [`Lexer`] from a source string.
    ///
    /// The lexer is positioned at the first character of the input.
    /// Call [`next_token`](Lexer::next_token) repeatedly or
    /// [`tokenize`](Lexer::tokenize) to consume all tokens.
    pub fn new(input: &str) -> Self {
        let chars: Vec<char> = input.chars().collect();
        let first = chars.first().cloned();
        Lexer {
            input: chars,
            position: 0,
            current_char: first,
            line: 1,
            col: 1,
        }
    }

    // ── Movement helpers ────────────────────────────────────────

    /// Advances the read position by one character, updating line
    /// and column counters.
    fn advance(&mut self) {
        if let Some(c) = self.current_char {
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        self.position += 1;
        self.current_char = self.input.get(self.position).cloned();
    }

    /// Returns the next character without advancing the position.
    fn peek(&self) -> Option<char> {
        self.input.get(self.position + 1).cloned()
    }

    /// Returns the character `n` positions ahead without advancing.
    #[allow(dead_code)]
    fn peek_n(&self, n: usize) -> Option<char> {
        self.input.get(self.position + n).cloned()
    }

    // ── Whitespace & Comments ───────────────────────────────────

    /// Skips over whitespace, block comments `(* ... *)`, and line
    /// comments `// ...`.
    ///
    /// Block comments may be nested. An unterminated block comment
    /// consumes to end-of-input without emitting an error token;
    /// the parser is responsible for reporting this.
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while let Some(c) = self.current_char {
                if c.is_ascii_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }

            // Block comment: (* ... *) — supports nesting
            if self.current_char == Some('(') && self.peek() == Some('*') {
                self.advance();
                self.advance();
                let mut depth = 1;
                while depth > 0 {
                    match self.current_char {
                        None => break,
                        Some('(') if self.peek() == Some('*') => {
                            depth += 1;
                            self.advance();
                            self.advance();
                        }
                        Some('*') if self.peek() == Some(')') => {
                            depth -= 1;
                            self.advance();
                            self.advance();
                        }
                        _ => self.advance(),
                    }
                }
                continue;
            }

            // Line comment: // ... \n
            if self.current_char == Some('/') && self.peek() == Some('/') {
                while let Some(c) = self.current_char {
                    if c == '\n' {
                        break;
                    }
                    self.advance();
                }
                continue;
            }

            break;
        }
    }

    // ── Identifier / Keyword scanner ────────────────────────────

    /// Scans an identifier or keyword starting at the current position.
    ///
    /// IEC 61131-3 keywords are case-insensitive; the match is
    /// performed on the uppercased form of the scanned text, while
    /// the original casing is preserved in [`Token::text`].
    fn scan_identifier(&mut self) -> Token {
        let start_line = self.line;
        let start_col = self.col;
        let start_pos = self.position;

        while let Some(c) = self.current_char {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let text: String = self.input[start_pos..self.position].iter().collect();
        let upper = text.to_ascii_uppercase();

        let kind = match upper.as_str() {
            "PROGRAM"            => TokenType::Program,
            "END_PROGRAM"        => TokenType::EndProgram,
            "FUNCTION"           => TokenType::Function,
            "END_FUNCTION"       => TokenType::EndFunction,
            "FUNCTION_BLOCK"     => TokenType::FunctionBlock,
            "END_FUNCTION_BLOCK" => TokenType::EndFunctionBlock,

            "VAR"                => TokenType::Var,
            "END_VAR"            => TokenType::EndVar,
            "VAR_INPUT"          => TokenType::VarInput,
            "VAR_OUTPUT"         => TokenType::VarOutput,
            "VAR_IN_OUT"         => TokenType::VarInOut,
            "VAR_GLOBAL"         => TokenType::VarGlobal,
            "VAR_EXTERNAL"       => TokenType::VarExternal,
            "VAR_TEMP"           => TokenType::VarTemp,
            "RETAIN"             => TokenType::Retain,
            "CONSTANT"           => TokenType::Constant,
            "AT"                 => TokenType::At,

            "BOOL"               => TokenType::TypeBool,
            "SINT"               => TokenType::TypeSint,
            "INT"                => TokenType::TypeInt,
            "DINT"               => TokenType::TypeDint,
            "LINT"               => TokenType::TypeLint,
            "USINT"              => TokenType::TypeUsint,
            "UINT"               => TokenType::TypeUint,
            "UDINT"              => TokenType::TypeUdint,
            "ULINT"              => TokenType::TypeUlint,
            "REAL"               => TokenType::TypeReal,
            "LREAL"              => TokenType::TypeLreal,
            "BYTE"               => TokenType::TypeByte,
            "WORD"               => TokenType::TypeWord,
            "DWORD"              => TokenType::TypeDword,
            "LWORD"              => TokenType::TypeLword,
            "STRING"             => TokenType::TypeString,
            "WSTRING"            => TokenType::TypeWstring,
            "TIME"               => TokenType::TypeTime,
            "DATE"               => TokenType::TypeDate,
            "TIME_OF_DAY" | "TOD" => TokenType::TypeTod,
            "DATE_AND_TIME" | "DT" => TokenType::TypeDt,

            "TYPE"               => TokenType::Type,
            "END_TYPE"           => TokenType::EndType,
            "STRUCT"             => TokenType::Struct,
            "END_STRUCT"         => TokenType::EndStruct,
            "ARRAY"              => TokenType::Array,
            "OF"                 => TokenType::Of,

            "IF"                 => TokenType::If,
            "THEN"               => TokenType::Then,
            "ELSIF"              => TokenType::Elsif,
            "ELSE"               => TokenType::Else,
            "END_IF"             => TokenType::EndIf,
            "CASE"               => TokenType::Case,
            "END_CASE"           => TokenType::EndCase,
            "FOR"                => TokenType::For,
            "TO"                 => TokenType::To,
            "BY"                 => TokenType::By,
            "DO"                 => TokenType::Do,
            "END_FOR"            => TokenType::EndFor,
            "WHILE"              => TokenType::While,
            "END_WHILE"          => TokenType::EndWhile,
            "REPEAT"             => TokenType::Repeat,
            "UNTIL"              => TokenType::Until,
            "END_REPEAT"         => TokenType::EndRepeat,
            "EXIT"               => TokenType::Exit,
            "RETURN"             => TokenType::Return,

            "AND"                => TokenType::And,
            "OR"                 => TokenType::Or,
            "XOR"                => TokenType::Xor,
            "NOT"                => TokenType::Not,
            "MOD"                => TokenType::Mod,

            "TRUE"               => TokenType::BoolLiteral,
            "FALSE"              => TokenType::BoolLiteral,

            _ => TokenType::Ident,
        };

        Token { kind, text, line: start_line, col: start_col }
    }

    // ── Numeric literal scanner ─────────────────────────────────

    /// Scans an integer or real literal starting at the current position.
    ///
    /// Handles the following forms:
    /// - Decimal integers: `42`, `1_000`
    /// - Based integers: `16#FF`, `8#77`, `2#1010_1100`
    /// - Real literals: `3.14`, `1.0e-3`, `0.5E+2`
    ///
    /// Leading signs (`+`/`-`) are **not** consumed — the parser
    /// treats those as unary operators. Trailing `#` for typed
    /// literals (e.g. `INT#5`) is also left for the parser; the
    /// lexer emits `TypeInt`, `Hash`, `IntLiteral` as three
    /// separate tokens.
    fn scan_number(&mut self) -> Token {
        let start_line = self.line;
        let start_col = self.col;
        let start_pos = self.position;
        let mut is_real = false;

        self.eat_digits();

        // Check for base prefix: 16#, 8#, 2#
        if self.current_char == Some('#') && self.position > start_pos {
            let prefix: String = self.input[start_pos..self.position].iter().collect();
            if prefix == "16" || prefix == "8" || prefix == "2" {
                self.advance();
                self.eat_hex_digits();
                let text: String = self.input[start_pos..self.position].iter().collect();
                return Token { kind: TokenType::IntLiteral, text, line: start_line, col: start_col };
            }
        }

        // Decimal point → real literal (but not '..' range operator)
        if self.current_char == Some('.') && self.peek().map_or(false, |c| c.is_ascii_digit()) {
            if self.peek() != Some('.') {
                is_real = true;
                self.advance();
                self.eat_digits();
            }
        }

        // Exponent part
        if self.current_char == Some('e') || self.current_char == Some('E') {
            is_real = true;
            self.advance();
            if self.current_char == Some('+') || self.current_char == Some('-') {
                self.advance();
            }
            self.eat_digits();
        }

        let text: String = self.input[start_pos..self.position].iter().collect();
        let kind = if is_real { TokenType::RealLiteral } else { TokenType::IntLiteral };
        Token { kind, text, line: start_line, col: start_col }
    }

    /// Consumes consecutive decimal digits and underscores.
    fn eat_digits(&mut self) {
        while let Some(c) = self.current_char {
            if c.is_ascii_digit() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Consumes consecutive hexadecimal digits and underscores.
    /// This is a superset that also works for base-2 and base-8
    /// literals; out-of-range digits are caught during semantic
    /// analysis rather than lexing.
    fn eat_hex_digits(&mut self) {
        while let Some(c) = self.current_char {
            if c.is_ascii_hexdigit() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
    }

    // ── String literal scanner ──────────────────────────────────

    /// Scans a string or wide-string literal.
    ///
    /// IEC 61131-3 uses single quotes for single-byte strings and
    /// double quotes for wide strings. The escape character is `$`,
    /// supporting sequences like `$$`, `$'`, `$"`, `$L`, `$N`,
    /// `$R`, `$T`, and `$xx` (hex byte).
    fn scan_string(&mut self, quote: char) -> Token {
        let start_line = self.line;
        let start_col = self.col;
        let start_pos = self.position;

        self.advance();

        while let Some(c) = self.current_char {
            if c == '$' {
                self.advance();
                if self.current_char.is_some() {
                    self.advance();
                }
            } else if c == quote {
                self.advance();
                break;
            } else {
                self.advance();
            }
        }

        let text: String = self.input[start_pos..self.position].iter().collect();
        let kind = if quote == '\'' {
            TokenType::StringLiteral
        } else {
            TokenType::WStringLiteral
        };
        Token { kind, text, line: start_line, col: start_col }
    }

    // ── Time / Date literal scanner ─────────────────────────────

    /// Scans a temporal literal after the prefix keyword and `#`
    /// have been identified.
    ///
    /// Called when the lexer has already scanned a keyword like `T`,
    /// `TIME`, `D`, `DATE`, `TOD`, or `DT` and the next character
    /// is `#`. Consumes the `#` and the literal value
    /// (digits, letters, colons, hyphens, dots, underscores).
    fn scan_temporal_literal(&mut self, prefix: &str) -> Token {
        let start_line = self.line;
        let start_col = self.col - prefix.len();
        let start_pos = self.position - prefix.len();

        self.advance();

        while let Some(c) = self.current_char {
            if c.is_ascii_alphanumeric() || c == ':' || c == '-' || c == '.' || c == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let text: String = self.input[start_pos..self.position].iter().collect();
        let upper_prefix = prefix.to_ascii_uppercase();
        let kind = match upper_prefix.as_str() {
            "T" | "TIME"                    => TokenType::TimeLiteral,
            "D" | "DATE"                    => TokenType::DateLiteral,
            "TOD" | "TIME_OF_DAY"           => TokenType::TodLiteral,
            "DT" | "DATE_AND_TIME"          => TokenType::DtLiteral,
            _ => TokenType::Ident,
        };
        Token { kind, text, line: start_line, col: start_col }
    }

    // ── Main tokeniser entry point ──────────────────────────────

    /// Scans and returns the next token from the input.
    ///
    /// Whitespace and comments are skipped automatically. Returns
    /// [`TokenType::Eof`] when the input is exhausted.
    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace_and_comments();

        let c = match self.current_char {
            Some(ch) => ch,
            None => return Token {
                kind: TokenType::Eof,
                text: String::new(),
                line: self.line,
                col: self.col,
            },
        };

        let start_line = self.line;
        let start_col = self.col;

        // ── Identifiers, keywords, and temporal literal prefixes ──
        if c.is_ascii_alphabetic() || c == '_' {
            let tok = self.scan_identifier();

            let upper = tok.text.to_ascii_uppercase();
            if self.current_char == Some('#') {
                match upper.as_str() {
                    "T" | "TIME" | "D" | "DATE" | "TOD" | "TIME_OF_DAY" | "DT" | "DATE_AND_TIME" => {
                        return self.scan_temporal_literal(&tok.text);
                    }
                    _ => {}
                }
            }
            return tok;
        }

        // ── Numeric literals ──
        if c.is_ascii_digit() {
            return self.scan_number();
        }

        // ── String literals ──
        if c == '\'' {
            return self.scan_string('\'');
        }
        if c == '"' {
            return self.scan_string('"');
        }

        // ── Multi-character operators ──

        if c == ':' && self.peek() == Some('=') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::Assignment, text: ":=".into(), line: start_line, col: start_col };
        }

        if c == '=' && self.peek() == Some('>') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::OutputAssign, text: "=>".into(), line: start_line, col: start_col };
        }

        if c == '*' && self.peek() == Some('*') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::Power, text: "**".into(), line: start_line, col: start_col };
        }

        if c == '<' && self.peek() == Some('>') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::NotEqual, text: "<>".into(), line: start_line, col: start_col };
        }

        if c == '<' && self.peek() == Some('=') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::LessEq, text: "<=".into(), line: start_line, col: start_col };
        }

        if c == '>' && self.peek() == Some('=') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::GreaterEq, text: ">=".into(), line: start_line, col: start_col };
        }

        if c == '-' && self.peek() == Some('>') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::Arrow, text: "->".into(), line: start_line, col: start_col };
        }

        if c == '.' && self.peek() == Some('.') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::DotDot, text: "..".into(), line: start_line, col: start_col };
        }

        // ── Single-character tokens ──
        self.advance();
        let text = c.to_string();

        let kind = match c {
            '+' => TokenType::Plus,
            '-' => TokenType::Minus,
            '*' => TokenType::Star,
            '/' => TokenType::Slash,
            '=' => TokenType::Equal,
            '<' => TokenType::Less,
            '>' => TokenType::Greater,
            '(' => TokenType::LParen,
            ')' => TokenType::RParen,
            '[' => TokenType::LBracket,
            ']' => TokenType::RBracket,
            ':' => TokenType::Colon,
            ';' => TokenType::SemiColon,
            ',' => TokenType::Comma,
            '.' => TokenType::Dot,
            '#' => TokenType::Hash,
            '&' => TokenType::Ampersand,
            _   => TokenType::Unknown,
        };

        Token { kind, text, line: start_line, col: start_col }
    }

    // ── Convenience: collect all tokens ─────────────────────────

    /// Consumes the entire input and returns all tokens as a vector.
    ///
    /// The returned vector always ends with a [`TokenType::Eof`]
    /// token. Whitespace and comments are skipped automatically.
    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            let is_eof = tok.kind == TokenType::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        tokens
    }
}

// ─── Unit Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: lex source and return vec of (TokenType, text).
    fn lex(src: &str) -> Vec<(TokenType, String)> {
        Lexer::new(src)
            .tokenize()
            .into_iter()
            .map(|t| (t.kind, t.text.clone()))
            .collect()
    }

    #[test]
    fn test_original_example() {
        let tokens = lex("PROGRAM MyFirstPLC VAR count : INT := 0; END_VAR");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::Program, TokenType::Ident, TokenType::Var,
            TokenType::Ident, TokenType::Colon, TokenType::TypeInt,
            TokenType::Assignment, TokenType::IntLiteral,
            TokenType::SemiColon, TokenType::EndVar, TokenType::Eof,
        ]);
    }

    #[test]
    fn test_case_insensitivity() {
        let tokens = lex("program var int end_var end_program");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::Program, TokenType::Var, TokenType::TypeInt,
            TokenType::EndVar, TokenType::EndProgram, TokenType::Eof,
        ]);
    }

    #[test]
    fn test_block_comment() {
        let tokens = lex("VAR (* this is a comment *) END_VAR");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![TokenType::Var, TokenType::EndVar, TokenType::Eof]);
    }

    #[test]
    fn test_nested_block_comment() {
        let tokens = lex("VAR (* outer (* inner *) still comment *) END_VAR");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![TokenType::Var, TokenType::EndVar, TokenType::Eof]);
    }

    #[test]
    fn test_line_comment() {
        let tokens = lex("VAR // this is ignored\nEND_VAR");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![TokenType::Var, TokenType::EndVar, TokenType::Eof]);
    }

    #[test]
    fn test_comparison_operators() {
        let tokens = lex("a < b <= c > d >= e = f <> g");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::Ident, TokenType::Less,
            TokenType::Ident, TokenType::LessEq,
            TokenType::Ident, TokenType::Greater,
            TokenType::Ident, TokenType::GreaterEq,
            TokenType::Ident, TokenType::Equal,
            TokenType::Ident, TokenType::NotEqual,
            TokenType::Ident, TokenType::Eof,
        ]);
    }

    #[test]
    fn test_multi_char_operators() {
        let tokens = lex(":= => ** -> ..");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::Assignment, TokenType::OutputAssign,
            TokenType::Power, TokenType::Arrow,
            TokenType::DotDot, TokenType::Eof,
        ]);
    }

    #[test]
    fn test_boolean_and_keyword_operators() {
        let tokens = lex("TRUE AND FALSE OR NOT XOR MOD");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::BoolLiteral, TokenType::And,
            TokenType::BoolLiteral, TokenType::Or,
            TokenType::Not, TokenType::Xor, TokenType::Mod,
            TokenType::Eof,
        ]);
    }

    #[test]
    fn test_real_literals() {
        let tokens = lex("3.14 1.0e-3 42.0E+2");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::RealLiteral, TokenType::RealLiteral,
            TokenType::RealLiteral, TokenType::Eof,
        ]);
        assert_eq!(tokens[0].1, "3.14");
        assert_eq!(tokens[1].1, "1.0e-3");
        assert_eq!(tokens[2].1, "42.0E+2");
    }

    #[test]
    fn test_based_integers() {
        let tokens = lex("16#FF 8#77 2#1010");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::IntLiteral, TokenType::IntLiteral,
            TokenType::IntLiteral, TokenType::Eof,
        ]);
        assert_eq!(tokens[0].1, "16#FF");
        assert_eq!(tokens[1].1, "8#77");
        assert_eq!(tokens[2].1, "2#1010");
    }

    #[test]
    fn test_underscores_in_numbers() {
        let tokens = lex("1_000 2#1010_1100");
        assert_eq!(tokens[0].1, "1_000");
        assert_eq!(tokens[0].0, TokenType::IntLiteral);
        assert_eq!(tokens[1].1, "2#1010_1100");
    }

    #[test]
    fn test_string_literal() {
        let tokens = lex("'hello world'");
        assert_eq!(tokens[0].0, TokenType::StringLiteral);
        assert_eq!(tokens[0].1, "'hello world'");
    }

    #[test]
    fn test_string_escape() {
        let tokens = lex("'it$$s a $'test$''");
        assert_eq!(tokens[0].0, TokenType::StringLiteral);
    }

    #[test]
    fn test_wstring_literal() {
        let tokens = lex("\"wide string\"");
        assert_eq!(tokens[0].0, TokenType::WStringLiteral);
    }

    #[test]
    fn test_time_literal() {
        let tokens = lex("T#5s TIME#1h2m3s");
        assert_eq!(tokens[0].0, TokenType::TimeLiteral);
        assert_eq!(tokens[0].1, "T#5s");
        assert_eq!(tokens[1].0, TokenType::TimeLiteral);
        assert_eq!(tokens[1].1, "TIME#1h2m3s");
    }

    #[test]
    fn test_date_literal() {
        let tokens = lex("D#2025-12-01 DATE#2025-12-01");
        assert_eq!(tokens[0].0, TokenType::DateLiteral);
        assert_eq!(tokens[1].0, TokenType::DateLiteral);
    }

    #[test]
    fn test_tod_literal() {
        let tokens = lex("TOD#14:30:00");
        assert_eq!(tokens[0].0, TokenType::TodLiteral);
    }

    #[test]
    fn test_dt_literal() {
        let tokens = lex("DT#2025-12-01-14:30:00");
        assert_eq!(tokens[0].0, TokenType::DtLiteral);
    }

    #[test]
    fn test_typed_literal_produces_separate_tokens() {
        let tokens = lex("INT#5");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::TypeInt, TokenType::Hash,
            TokenType::IntLiteral, TokenType::Eof,
        ]);
    }

    #[test]
    fn test_control_flow_keywords() {
        let tokens = lex("IF x THEN y := 1; ELSIF z THEN y := 2; ELSE y := 3; END_IF");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert!(kinds.contains(&TokenType::If));
        assert!(kinds.contains(&TokenType::Then));
        assert!(kinds.contains(&TokenType::Elsif));
        assert!(kinds.contains(&TokenType::Else));
        assert!(kinds.contains(&TokenType::EndIf));
    }

    #[test]
    fn test_for_loop() {
        let tokens = lex("FOR i := 0 TO 10 BY 2 DO x := x + i; END_FOR");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert!(kinds.contains(&TokenType::For));
        assert!(kinds.contains(&TokenType::To));
        assert!(kinds.contains(&TokenType::By));
        assert!(kinds.contains(&TokenType::Do));
        assert!(kinds.contains(&TokenType::EndFor));
    }

    #[test]
    fn test_while_loop() {
        let tokens = lex("WHILE x > 0 DO x := x - 1; END_WHILE");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert!(kinds.contains(&TokenType::While));
        assert!(kinds.contains(&TokenType::Do));
        assert!(kinds.contains(&TokenType::EndWhile));
    }

    #[test]
    fn test_repeat_loop() {
        let tokens = lex("REPEAT x := x + 1; UNTIL x >= 10 END_REPEAT");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert!(kinds.contains(&TokenType::Repeat));
        assert!(kinds.contains(&TokenType::Until));
        assert!(kinds.contains(&TokenType::EndRepeat));
    }

    #[test]
    fn test_case_statement() {
        let tokens = lex("CASE state OF 0: x := 1; 1..5: x := 2; END_CASE");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert!(kinds.contains(&TokenType::Case));
        assert!(kinds.contains(&TokenType::Of));
        assert!(kinds.contains(&TokenType::DotDot));
        assert!(kinds.contains(&TokenType::EndCase));
    }

    #[test]
    fn test_array_declaration() {
        let tokens = lex("arr : ARRAY[0..9] OF INT");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::Ident, TokenType::Colon, TokenType::Array,
            TokenType::LBracket, TokenType::IntLiteral, TokenType::DotDot,
            TokenType::IntLiteral, TokenType::RBracket,
            TokenType::Of, TokenType::TypeInt, TokenType::Eof,
        ]);
    }

    #[test]
    fn test_function_block() {
        let tokens = lex("FUNCTION_BLOCK MyFB VAR_INPUT x : REAL; END_VAR END_FUNCTION_BLOCK");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert!(kinds.contains(&TokenType::FunctionBlock));
        assert!(kinds.contains(&TokenType::VarInput));
        assert!(kinds.contains(&TokenType::TypeReal));
        assert!(kinds.contains(&TokenType::EndFunctionBlock));
    }

    #[test]
    fn test_all_data_types() {
        let src = "BOOL SINT INT DINT LINT USINT UINT UDINT ULINT REAL LREAL BYTE WORD DWORD LWORD STRING WSTRING TIME DATE";
        let tokens = lex(src);
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::TypeBool, TokenType::TypeSint, TokenType::TypeInt,
            TokenType::TypeDint, TokenType::TypeLint, TokenType::TypeUsint,
            TokenType::TypeUint, TokenType::TypeUdint, TokenType::TypeUlint,
            TokenType::TypeReal, TokenType::TypeLreal, TokenType::TypeByte,
            TokenType::TypeWord, TokenType::TypeDword, TokenType::TypeLword,
            TokenType::TypeString, TokenType::TypeWstring, TokenType::TypeTime,
            TokenType::TypeDate, TokenType::Eof,
        ]);
    }

    #[test]
    fn test_line_tracking() {
        let tokens = lex("VAR\n  x : INT;\nEND_VAR");
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[1].line, 2);
        assert_eq!(tokens[4].line, 2);
        assert_eq!(tokens[5].line, 3);
    }

    #[test]
    fn test_realistic_program() {
        let src = r#"
PROGRAM ConveyorControl
VAR
    speed : REAL := 0.0;
    running : BOOL := FALSE;
    count : INT := 0;
    limit : INT := 1_000;
END_VAR

(* Main control logic *)
IF running AND speed > 0.0 THEN
    count := count + 1;
    IF count >= limit THEN
        running := FALSE; // safety stop
        speed := 0.0;
    END_IF;
END_IF;

END_PROGRAM
"#;
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();
        for t in &tokens {
            assert_ne!(t.kind, TokenType::Unknown, "Unknown token: '{}' at {}:{}", t.text, t.line, t.col);
        }
        assert_eq!(tokens.first().unwrap().kind, TokenType::Program);
        assert_eq!(tokens[tokens.len() - 2].kind, TokenType::EndProgram);
        assert_eq!(tokens.last().unwrap().kind, TokenType::Eof);
    }
}