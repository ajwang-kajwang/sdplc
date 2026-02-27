// src/lexer.rs
//
// IEC 61131-3 Structured Text Lexer
// Covers keywords, data types, operators, literals, and comments
// per IEC 61131-3:2025 (4th edition, IL removed)

// ─── Token Types ────────────────────────────────────────────────

/// Represents the specific type of the token.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TokenType {
    // ── Special ──
    Eof,
    Unknown,

    // ── Identifiers & Literals ──
    Ident,          // Variable/function names
    IntLiteral,     // 42, 16#FF, 8#77, 2#1010, 1_000
    RealLiteral,    // 3.14, 1.0e-3
    StringLiteral,  // 'hello world'
    WStringLiteral, // "hello world" (double-quoted wide string)
    TimeLiteral,    // T#5s, TIME#1h2m3s
    DateLiteral,    // D#2025-12-01
    TodLiteral,     // TOD#14:30:00
    DtLiteral,      // DT#2025-12-01-14:30:00
    BoolLiteral,    // TRUE, FALSE

    // ── Program Organisation Units ──
    Program,        // PROGRAM
    EndProgram,     // END_PROGRAM
    Function,       // FUNCTION
    EndFunction,    // END_FUNCTION
    FunctionBlock,  // FUNCTION_BLOCK
    EndFunctionBlock, // END_FUNCTION_BLOCK

    // ── Variable Declarations ──
    Var,            // VAR
    EndVar,         // END_VAR
    VarInput,       // VAR_INPUT
    EndVarInput,    // Reuses EndVar
    VarOutput,      // VAR_OUTPUT
    VarInOut,       // VAR_IN_OUT
    VarGlobal,      // VAR_GLOBAL
    VarExternal,    // VAR_EXTERNAL
    VarTemp,        // VAR_TEMP
    Retain,         // RETAIN
    Constant,       // CONSTANT
    At,             // AT (direct address)

    // ── Data Type Keywords ──
    TypeBool,       // BOOL
    TypeSint,       // SINT
    TypeInt,        // INT
    TypeDint,       // DINT
    TypeLint,       // LINT
    TypeUsint,      // USINT
    TypeUint,       // UINT
    TypeUdint,      // UDINT
    TypeUlint,      // ULINT
    TypeReal,       // REAL
    TypeLreal,      // LREAL
    TypeByte,       // BYTE
    TypeWord,       // WORD
    TypeDword,      // DWORD
    TypeLword,      // LWORD
    TypeString,     // STRING
    TypeWstring,    // WSTRING
    TypeTime,       // TIME
    TypeDate,       // DATE
    TypeTod,        // TIME_OF_DAY / TOD
    TypeDt,         // DATE_AND_TIME / DT

    // ── User-Defined Types ──
    Type,           // TYPE
    EndType,        // END_TYPE
    Struct,         // STRUCT
    EndStruct,      // END_STRUCT
    Array,          // ARRAY
    Of,             // OF

    // ── Control Flow ──
    If,             // IF
    Then,           // THEN
    Elsif,          // ELSIF
    Else,           // ELSE
    EndIf,          // END_IF
    Case,           // CASE
    EndCase,        // END_CASE
    For,            // FOR
    To,             // TO
    By,             // BY
    Do,             // DO
    EndFor,         // END_FOR
    While,          // WHILE
    EndWhile,       // END_WHILE
    Repeat,         // REPEAT
    Until,          // UNTIL
    EndRepeat,      // END_REPEAT
    Exit,           // EXIT
    Return,         // RETURN

    // ── Boolean / Bitwise Operators (keyword form) ──
    And,            // AND / &
    Or,             // OR
    Xor,            // XOR
    Not,            // NOT
    Mod,            // MOD

    // ── Arithmetic Operators ──
    Plus,           // +
    Minus,          // -
    Star,           // *
    Slash,          // /
    Power,          // **

    // ── Comparison Operators ──
    Equal,          // =
    NotEqual,       // <>
    Less,           // <
    LessEq,         // <=
    Greater,        // >
    GreaterEq,      // >=

    // ── Assignment & Connectors ──
    Assignment,     // :=
    OutputAssign,   // =>
    Arrow,          // ->   (SFC transition)
    
    // ── Delimiters ──
    Colon,          // :
    SemiColon,      // ;
    Comma,          // ,
    Dot,            // .
    DotDot,         // ..   (range for ARRAY, CASE)
    LParen,         // (
    RParen,         // )
    LBracket,       // [
    RBracket,       // ]
    Hash,           // #    (typed literal prefix: INT#5)
    Ampersand,      // &    (alternate AND)
}

/// A single token with its classification, original text, and source location.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenType,
    pub text: String,
    pub line: usize,
    pub col: usize,
}

// ─── Lexer ──────────────────────────────────────────────────────

pub struct Lexer {
    input: Vec<char>,
    position: usize,
    current_char: Option<char>,
    line: usize,
    col: usize,
}

impl Lexer {
    /// Creates a new Lexer from a source string.
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

    fn peek(&self) -> Option<char> {
        self.input.get(self.position + 1).cloned()
    }

    fn peek_n(&self, n: usize) -> Option<char> {
        self.input.get(self.position + n).cloned()
    }

    // ── Whitespace & Comments ───────────────────────────────────

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while let Some(c) = self.current_char {
                if c.is_ascii_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }

            // Block comment: (* ... *)
            if self.current_char == Some('(') && self.peek() == Some('*') {
                self.advance(); // consume '('
                self.advance(); // consume '*'
                let mut depth = 1;
                while depth > 0 {
                    match self.current_char {
                        None => break, // unterminated comment — parser will catch it
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
            // Program Organisation Units
            "PROGRAM"            => TokenType::Program,
            "END_PROGRAM"        => TokenType::EndProgram,
            "FUNCTION"           => TokenType::Function,
            "END_FUNCTION"       => TokenType::EndFunction,
            "FUNCTION_BLOCK"     => TokenType::FunctionBlock,
            "END_FUNCTION_BLOCK" => TokenType::EndFunctionBlock,

            // Variable sections
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

            // Data types
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

            // User-defined types
            "TYPE"               => TokenType::Type,
            "END_TYPE"           => TokenType::EndType,
            "STRUCT"             => TokenType::Struct,
            "END_STRUCT"         => TokenType::EndStruct,
            "ARRAY"              => TokenType::Array,
            "OF"                 => TokenType::Of,

            // Control flow
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

            // Boolean / bitwise operators
            "AND"                => TokenType::And,
            "OR"                 => TokenType::Or,
            "XOR"                => TokenType::Xor,
            "NOT"                => TokenType::Not,
            "MOD"                => TokenType::Mod,

            // Boolean literals
            "TRUE"               => TokenType::BoolLiteral,
            "FALSE"              => TokenType::BoolLiteral,

            _ => TokenType::Ident,
        };

        Token { kind, text, line: start_line, col: start_col }
    }

    // ── Numeric literal scanner ─────────────────────────────────
    //
    // Handles:  42   1_000   16#FF   8#77   2#1010_1100
    //           3.14   1.0e-3   0.5E+2
    //
    // Does NOT consume a leading sign — the parser handles unary +/-.
    // Does NOT consume a trailing # for typed literals (INT#5) —
    // the parser sees Ident(INT) Hash IntLiteral(5).

    fn scan_number(&mut self) -> Token {
        let start_line = self.line;
        let start_col = self.col;
        let start_pos = self.position;
        let mut is_real = false;

        // Consume leading digits (possibly with underscores)
        self.eat_digits();

        // Check for base prefix: 16#, 8#, 2#
        if self.current_char == Some('#') && self.position > start_pos {
            let prefix: String = self.input[start_pos..self.position].iter().collect();
            if prefix == "16" || prefix == "8" || prefix == "2" {
                self.advance(); // consume '#'
                self.eat_hex_digits(); // hex covers all bases
                let text: String = self.input[start_pos..self.position].iter().collect();
                return Token { kind: TokenType::IntLiteral, text, line: start_line, col: start_col };
            }
        }

        // Decimal point → real literal
        if self.current_char == Some('.') && self.peek().map_or(false, |c| c.is_ascii_digit()) {
            // Make sure it's not '..' (range)
            if self.peek() != Some('.') {
                is_real = true;
                self.advance(); // consume '.'
                self.eat_digits();
            }
        }

        // Exponent
        if self.current_char == Some('e') || self.current_char == Some('E') {
            is_real = true;
            self.advance(); // consume 'e'/'E'
            if self.current_char == Some('+') || self.current_char == Some('-') {
                self.advance(); // consume sign
            }
            self.eat_digits();
        }

        let text: String = self.input[start_pos..self.position].iter().collect();
        let kind = if is_real { TokenType::RealLiteral } else { TokenType::IntLiteral };
        Token { kind, text, line: start_line, col: start_col }
    }

    /// Eat decimal digits and underscores
    fn eat_digits(&mut self) {
        while let Some(c) = self.current_char {
            if c.is_ascii_digit() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Eat hex digits and underscores (superset — works for base 2, 8, 16)
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
    //
    // IEC 61131-3 single-byte strings use single quotes: 'abc'
    // Wide strings use double quotes: "abc"
    // Escape sequences: $$ $' $" $L $N $R $T $xx

    fn scan_string(&mut self, quote: char) -> Token {
        let start_line = self.line;
        let start_col = self.col;
        let start_pos = self.position;

        self.advance(); // consume opening quote

        while let Some(c) = self.current_char {
            if c == '$' {
                // IEC 61131-3 escape — skip the next char
                self.advance();
                if self.current_char.is_some() {
                    self.advance();
                }
            } else if c == quote {
                self.advance(); // consume closing quote
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
    //
    // Called after we've already scanned a keyword (TIME, T, DATE, D,
    // TOD, DT) and see '#' as the next character.
    // We consume '#' and everything up to the next whitespace/delimiter.

    fn scan_temporal_literal(&mut self, prefix: &str) -> Token {
        let start_line = self.line;
        let start_col = self.col - prefix.len();
        let start_pos = self.position - prefix.len();

        self.advance(); // consume '#'

        // Consume the literal value: digits, letters, colons, hyphens, dots, underscores
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
            _ => TokenType::Ident, // shouldn't happen
        };
        Token { kind, text, line: start_line, col: start_col }
    }

    // ── Main tokeniser entry point ──────────────────────────────

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

            // Check for temporal literal: keyword immediately followed by '#'
            // e.g. T#5s, TIME#1h2m, D#2025-01-01, TOD#14:30:00, DT#...
            let upper = tok.text.to_ascii_uppercase();
            if self.current_char == Some('#') {
                match upper.as_str() {
                    "T" | "TIME" | "D" | "DATE" | "TOD" | "TIME_OF_DAY" | "DT" | "DATE_AND_TIME" => {
                        return self.scan_temporal_literal(&tok.text);
                    }
                    _ => {} // Typed literal like INT#5 — return the ident, '#' comes next
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

        // :=  (assignment)
        if c == ':' && self.peek() == Some('=') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::Assignment, text: ":=".into(), line: start_line, col: start_col };
        }

        // =>  (output assignment / FB connection)
        if c == '=' && self.peek() == Some('>') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::OutputAssign, text: "=>".into(), line: start_line, col: start_col };
        }

        // **  (exponentiation)
        if c == '*' && self.peek() == Some('*') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::Power, text: "**".into(), line: start_line, col: start_col };
        }

        // <>  (not equal)
        if c == '<' && self.peek() == Some('>') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::NotEqual, text: "<>".into(), line: start_line, col: start_col };
        }

        // <=  (less than or equal)
        if c == '<' && self.peek() == Some('=') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::LessEq, text: "<=".into(), line: start_line, col: start_col };
        }

        // >=  (greater than or equal)
        if c == '>' && self.peek() == Some('=') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::GreaterEq, text: ">=".into(), line: start_line, col: start_col };
        }

        // ->  (SFC transition)
        if c == '-' && self.peek() == Some('>') {
            self.advance();
            self.advance();
            return Token { kind: TokenType::Arrow, text: "->".into(), line: start_line, col: start_col };
        }

        // ..  (range)
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

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: lex source and return vec of (TokenType, text)
    fn lex(src: &str) -> Vec<(TokenType, String)> {
        Lexer::new(src)
            .tokenize()
            .into_iter()
            .map(|t| (t.kind, t.text.clone()))
            .collect()
    }

    #[test]
    fn test_original_example() {
        // The test from your original main.rs
        let tokens = lex("PROGRAM MyFirstPLC VAR count : INT := 0; END_VAR");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::Program,
            TokenType::Ident,       // MyFirstPLC
            TokenType::Var,
            TokenType::Ident,       // count
            TokenType::Colon,
            TokenType::TypeInt,
            TokenType::Assignment,
            TokenType::IntLiteral,  // 0
            TokenType::SemiColon,
            TokenType::EndVar,
            TokenType::Eof,
        ]);
    }

    #[test]
    fn test_case_insensitivity() {
        // IEC 61131-3 keywords are case-insensitive
        let tokens = lex("program var int end_var end_program");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::Program,
            TokenType::Var,
            TokenType::TypeInt,
            TokenType::EndVar,
            TokenType::EndProgram,
            TokenType::Eof,
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
            TokenType::Assignment,
            TokenType::OutputAssign,
            TokenType::Power,
            TokenType::Arrow,
            TokenType::DotDot,
            TokenType::Eof,
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
            TokenType::RealLiteral,
            TokenType::RealLiteral,
            TokenType::RealLiteral,
            TokenType::Eof,
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
            TokenType::IntLiteral,
            TokenType::IntLiteral,
            TokenType::IntLiteral,
            TokenType::Eof,
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
        // INT#5 should be: TypeInt, Hash, IntLiteral
        let tokens = lex("INT#5");
        let kinds: Vec<TokenType> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![
            TokenType::TypeInt,
            TokenType::Hash,
            TokenType::IntLiteral,
            TokenType::Eof,
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
            TokenType::Ident,       // arr
            TokenType::Colon,
            TokenType::Array,
            TokenType::LBracket,
            TokenType::IntLiteral,  // 0
            TokenType::DotDot,
            TokenType::IntLiteral,  // 9
            TokenType::RBracket,
            TokenType::Of,
            TokenType::TypeInt,
            TokenType::Eof,
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
        assert_eq!(tokens[0].line, 1); // VAR
        assert_eq!(tokens[1].line, 2); // x
        assert_eq!(tokens[4].line, 2); // ;
        assert_eq!(tokens[5].line, 3); // END_VAR
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
        // Should parse without Unknown tokens (except EOF)
        for t in &tokens {
            assert_ne!(t.kind, TokenType::Unknown, "Unknown token: '{}' at {}:{}", t.text, t.line, t.col);
        }
        // Verify we got the right bookends
        assert_eq!(tokens.first().unwrap().kind, TokenType::Program);
        assert_eq!(tokens[tokens.len() - 2].kind, TokenType::EndProgram);
        assert_eq!(tokens.last().unwrap().kind, TokenType::Eof);
    }
}