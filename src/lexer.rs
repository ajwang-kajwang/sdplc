// src/lexer.rs

/// Represents the specific type of the token.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TokenType {
    // Special
    Eof,        // End of Input
    Unknown,    // Invalid character
    
    // Identifiers and Literals
    Ident,      // Variable names 
    Number,     // Integer literals 
    
    // Keywords (IEC 61131-3)
    Program,  // PROGRAM
    EndProg,  // END_PROGRAM
    Var,      // VAR
    EndVar,   // END_VAR
    If,       // IF
    Then,     // THEN
    EndIf,    // END_IF
    
    // Operators & Symbols
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Colon,      // :
    SemiColon,  // ;
    LParen,     // (
    RParen,     // )
    Assignment, // :=  
}

/// A single token with its classification and original text.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenType,
    pub text: String,
}

pub struct Lexer {
    /// The source code as a vector of characters (for easy indexing)
    input: Vec<char>,
    /// The current position in the input vector
    position: usize,
    /// The character currently being examined
    current_char: Option<char>,
}

impl Lexer {
    /// Creates a new Lexer instance from a source string.
    pub fn new(input: &str) -> Self {
        let chars: Vec<char> = input.chars().collect();
        Lexer {
            input: chars.clone(),
            position: 0,
            current_char: chars.first().cloned(),
        }
    }

    /// Advances the position pointer and updates current_char.
    fn advance(&mut self) {
        self.position += 1;
        if self.position < self.input.len() {
            self.current_char = Some(self.input[self.position]);
        } else {
            self.current_char = None;
        }
    }

    /// Peeks at the *next* character without moving the pointer.
    /// Detecting two-character operators like ':='
    fn peek(&self) -> Option<char> {
        if self.position + 1 < self.input.len() {
            Some(self.input[self.position + 1])
        } else {
            None
        }
    }

    /// Skips whitespace characters
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.current_char {
            if c.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Main function: Scans the input and returns the next Token.
    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();

        // 1. Check for End of File
        let c = match self.current_char {
            Some(ch) => ch,
            None => return Token { kind: TokenType::Eof, text: String::new() },
        };

        let start_pos = self.position;

        // 2. Handle Identifiers & Keywords (Starts with A-Z or a-z)
        
        if c.is_ascii_alphabetic() || c == '_' {
            while let Some(ch) = self.current_char {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
            
            let text: String = self.input[start_pos..self.position].iter().collect();
            
            // Map text to Keywords (Simple match, extendable list)
            let kind = match text.as_str() {
                "PROGRAM" => TokenType::Program,
                "END_PROGRAM" => TokenType::EndProg,
                "VAR" => TokenType::Var,
                "END_VAR" => TokenType::EndVar,
                "IF" => TokenType::If,
                "THEN" => TokenType::Then,
                "END_IF" => TokenType::EndIf,
                _ => TokenType::Ident,
            };

            return Token { kind, text };
        }

        // 3. Handle Numbers (Starts with 0-9)
        
        if c.is_ascii_digit() {
            while let Some(ch) = self.current_char {
                if ch.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
            let text: String = self.input[start_pos..self.position].iter().collect();
            return Token { kind: TokenType::Number, text };
        }

        // 4. Handle Symbols & Operators
        
        
        // SPECIAL CASE: The Assignment Operator (:=)
        if c == ':' {
            if self.peek() == Some('=') {
                self.advance(); // consume ':'
                self.advance(); // consume '='
                return Token { 
                    kind: TokenType::Assignment, 
                    text: ":=".to_string() 
                };
            }
        }

        // Standard Single-Character Tokens
        self.advance(); // Consume the character
        let text = c.to_string();
        
        let kind = match c {
            '+' => TokenType::Plus,
            '-' => TokenType::Minus,
            '*' => TokenType::Star,
            '/' => TokenType::Slash,
            '(' => TokenType::LParen,
            ')' => TokenType::RParen,
            ':' => TokenType::Colon, 
            ';' => TokenType::SemiColon,
            _   => TokenType::Unknown,
        };

        Token { kind, text }
    }
}