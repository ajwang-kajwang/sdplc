//! Recursive descent parser for IEC 61131-3 Structured Text.
//!
//! Consumes a token stream from the [`Lexer`](crate::lexer::Lexer) and
//! produces an [`AST`](crate::ast) representing the program structure.
//!
//! Expression parsing uses precedence climbing to correctly handle the
//! IEC 61131-3 operator precedence hierarchy (OR < XOR < AND < comparison
//! < addition < multiplication < exponentiation < unary < primary).

use crate::ast::*;
use crate::lexer::{Lexer, Token, TokenType};

// ─── Parse Errors ───────────────────────────────────────────────

/// An error encountered during parsing.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}:{}] {}", self.line, self.col, self.message)
    }
}

pub type ParseResult<T> = Result<T, ParseError>;

// ─── Parser ─────────────────────────────────────────────────────

/// A recursive descent parser for IEC 61131-3 Structured Text.
///
/// # Example
///
/// ```
/// use sdplc::lexer::Lexer;
/// use sdplc::parser::Parser;
///
/// let source = "PROGRAM P VAR x : INT := 0; END_VAR END_PROGRAM";
/// let lexer = Lexer::new(source);
/// let mut parser = Parser::new(lexer);
/// let unit = parser.parse().expect("parse error");
/// assert_eq!(unit.units.len(), 1);
/// ```
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    /// Creates a new parser from a lexer, consuming all tokens upfront.
    pub fn new(mut lexer: Lexer) -> Self {
        let tokens = lexer.tokenize();
        Parser { tokens, pos: 0 }
    }

    // ── Token navigation ────────────────────────────────────────

    /// Returns the current token without consuming it.
    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    /// Returns the type of the current token.
    fn current_kind(&self) -> TokenType {
        self.current().kind
    }

    /// Returns a Span for the current token.
    fn span(&self) -> Span {
        let t = self.current();
        Span { line: t.line, col: t.col }
    }

    /// Advances to the next token and returns the consumed token.
    fn advance(&mut self) -> &Token {
        let t = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    /// Consumes the current token if it matches `kind`, otherwise
    /// returns a parse error.
    fn expect(&mut self, kind: TokenType) -> ParseResult<Token> {
        if self.current_kind() == kind {
            Ok(self.advance().clone())
        } else {
            Err(self.error(format!(
                "expected {:?}, found {:?} '{}'",
                kind,
                self.current_kind(),
                self.current().text
            )))
        }
    }

    /// Consumes the current token if it matches, returns true.
    /// Otherwise returns false without consuming.
    fn eat(&mut self, kind: TokenType) -> bool {
        if self.current_kind() == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Constructs a ParseError at the current token position.
    fn error(&self, message: String) -> ParseError {
        let t = self.current();
        ParseError {
            message,
            line: t.line,
            col: t.col,
        }
    }

    // ── Top-level parsing ───────────────────────────────────────

    /// Parses the entire input as a compilation unit containing
    /// one or more POUs.
    pub fn parse(&mut self) -> ParseResult<CompilationUnit> {
        let mut units = Vec::new();

        while self.current_kind() != TokenType::Eof {
            match self.current_kind() {
                TokenType::Program => units.push(Pou::Program(self.parse_program()?)),
                TokenType::Function => units.push(Pou::Function(self.parse_function()?)),
                TokenType::FunctionBlock => units.push(Pou::FunctionBlock(self.parse_function_block()?)),
                _ => return Err(self.error(format!(
                    "expected PROGRAM, FUNCTION, or FUNCTION_BLOCK, found {:?} '{}'",
                    self.current_kind(),
                    self.current().text
                ))),
            }
        }

        Ok(CompilationUnit { units })
    }

    /// `PROGRAM name {var_blocks} {statements} END_PROGRAM`
    fn parse_program(&mut self) -> ParseResult<ProgramDecl> {
        let span = self.span();
        self.expect(TokenType::Program)?;
        let name = self.expect(TokenType::Ident)?.text;
        let var_blocks = self.parse_var_blocks()?;
        let body = self.parse_statement_list(&[TokenType::EndProgram])?;
        self.expect(TokenType::EndProgram)?;
        Ok(ProgramDecl { name, var_blocks, body, span })
    }

    /// `FUNCTION name : return_type {var_blocks} {statements} END_FUNCTION`
    fn parse_function(&mut self) -> ParseResult<FunctionDecl> {
        let span = self.span();
        self.expect(TokenType::Function)?;
        let name = self.expect(TokenType::Ident)?.text;
        self.expect(TokenType::Colon)?;
        let return_type = self.parse_type_spec()?;
        let var_blocks = self.parse_var_blocks()?;
        let body = self.parse_statement_list(&[TokenType::EndFunction])?;
        self.expect(TokenType::EndFunction)?;
        Ok(FunctionDecl { name, return_type, var_blocks, body, span })
    }

    /// `FUNCTION_BLOCK name {var_blocks} {statements} END_FUNCTION_BLOCK`
    fn parse_function_block(&mut self) -> ParseResult<FunctionBlockDecl> {
        let span = self.span();
        self.expect(TokenType::FunctionBlock)?;
        let name = self.expect(TokenType::Ident)?.text;
        let var_blocks = self.parse_var_blocks()?;
        let body = self.parse_statement_list(&[TokenType::EndFunctionBlock])?;
        self.expect(TokenType::EndFunctionBlock)?;
        Ok(FunctionBlockDecl { name, var_blocks, body, span })
    }

    // ── Variable declarations ───────────────────────────────────

    /// Parses zero or more consecutive VAR blocks.
    fn parse_var_blocks(&mut self) -> ParseResult<Vec<VarBlock>> {
        let mut blocks = Vec::new();
        loop {
            let qualifier = match self.current_kind() {
                TokenType::Var         => VarQualifier::Var,
                TokenType::VarInput    => VarQualifier::VarInput,
                TokenType::VarOutput   => VarQualifier::VarOutput,
                TokenType::VarInOut    => VarQualifier::VarInOut,
                TokenType::VarGlobal   => VarQualifier::VarGlobal,
                TokenType::VarExternal => VarQualifier::VarExternal,
                TokenType::VarTemp     => VarQualifier::VarTemp,
                _ => break,
            };
            blocks.push(self.parse_var_block(qualifier)?);
        }
        Ok(blocks)
    }

    /// Parses a single VAR block: `VAR [RETAIN] [CONSTANT] {decls} END_VAR`
    fn parse_var_block(&mut self, qualifier: VarQualifier) -> ParseResult<VarBlock> {
        let span = self.span();
        self.advance(); // consume VAR / VAR_INPUT / etc.

        let retain = self.eat(TokenType::Retain);
        let constant = self.eat(TokenType::Constant);

        let mut declarations = Vec::new();
        while self.current_kind() != TokenType::EndVar && self.current_kind() != TokenType::Eof {
            declarations.push(self.parse_var_decl()?);
        }
        self.expect(TokenType::EndVar)?;

        Ok(VarBlock { qualifier, retain, constant, declarations, span })
    }

    /// Parses a single declaration: `name : type_spec [:= init_value] ;`
    fn parse_var_decl(&mut self) -> ParseResult<VarDecl> {
        let span = self.span();
        let name = self.expect(TokenType::Ident)?.text;
        self.expect(TokenType::Colon)?;
        let type_spec = self.parse_type_spec()?;

        let initial_value = if self.eat(TokenType::Assignment) {
            Some(self.parse_expression()?)
        } else {
            None
        };

        self.expect(TokenType::SemiColon)?;
        Ok(VarDecl { name, type_spec, initial_value, span })
    }

    // ── Type specifications ─────────────────────────────────────

    /// Parses a type specifier.
    fn parse_type_spec(&mut self) -> ParseResult<TypeSpec> {
        match self.current_kind() {
            TokenType::Array => self.parse_array_type(),
            TokenType::TypeString => {
                self.advance();
                let max_len = if self.eat(TokenType::LBracket) {
                    let len = self.parse_integer_value()?;
                    self.expect(TokenType::RBracket)?;
                    Some(len as u32)
                } else {
                    None
                };
                Ok(TypeSpec::StringType { max_len })
            }
            TokenType::TypeWstring => {
                self.advance();
                let max_len = if self.eat(TokenType::LBracket) {
                    let len = self.parse_integer_value()?;
                    self.expect(TokenType::RBracket)?;
                    Some(len as u32)
                } else {
                    None
                };
                Ok(TypeSpec::WStringType { max_len })
            }
            _ => {
                if let Some(et) = self.try_elementary_type() {
                    self.advance();
                    Ok(TypeSpec::Elementary(et))
                } else if self.current_kind() == TokenType::Ident {
                    let name = self.advance().text.clone();
                    Ok(TypeSpec::UserDefined(name))
                } else {
                    Err(self.error(format!(
                        "expected type specifier, found {:?} '{}'",
                        self.current_kind(),
                        self.current().text
                    )))
                }
            }
        }
    }

    /// `ARRAY[lo..hi {, lo..hi}] OF element_type`
    fn parse_array_type(&mut self) -> ParseResult<TypeSpec> {
        self.expect(TokenType::Array)?;
        self.expect(TokenType::LBracket)?;

        let mut ranges = Vec::new();
        loop {
            let low = self.parse_integer_value()?;
            self.expect(TokenType::DotDot)?;
            let high = self.parse_integer_value()?;
            ranges.push(ArrayRange { low, high });
            if !self.eat(TokenType::Comma) {
                break;
            }
        }

        self.expect(TokenType::RBracket)?;
        self.expect(TokenType::Of)?;
        let element_type = Box::new(self.parse_type_spec()?);

        Ok(TypeSpec::Array { ranges, element_type })
    }

    /// Tries to map the current token to an ElementaryType.
    fn try_elementary_type(&self) -> Option<ElementaryType> {
        match self.current_kind() {
            TokenType::TypeBool  => Some(ElementaryType::Bool),
            TokenType::TypeSint  => Some(ElementaryType::Sint),
            TokenType::TypeInt   => Some(ElementaryType::Int),
            TokenType::TypeDint  => Some(ElementaryType::Dint),
            TokenType::TypeLint  => Some(ElementaryType::Lint),
            TokenType::TypeUsint => Some(ElementaryType::Usint),
            TokenType::TypeUint  => Some(ElementaryType::Uint),
            TokenType::TypeUdint => Some(ElementaryType::Udint),
            TokenType::TypeUlint => Some(ElementaryType::Ulint),
            TokenType::TypeReal  => Some(ElementaryType::Real),
            TokenType::TypeLreal => Some(ElementaryType::Lreal),
            TokenType::TypeByte  => Some(ElementaryType::Byte),
            TokenType::TypeWord  => Some(ElementaryType::Word),
            TokenType::TypeDword => Some(ElementaryType::Dword),
            TokenType::TypeLword => Some(ElementaryType::Lword),
            TokenType::TypeTime  => Some(ElementaryType::Time),
            TokenType::TypeDate  => Some(ElementaryType::Date),
            TokenType::TypeTod   => Some(ElementaryType::Tod),
            TokenType::TypeDt    => Some(ElementaryType::Dt),
            _ => None,
        }
    }

    /// Parses a compile-time integer value (for array bounds, etc.).
    /// Handles optional unary minus.
    fn parse_integer_value(&mut self) -> ParseResult<i64> {
        let negative = self.eat(TokenType::Minus);
        let tok = self.expect(TokenType::IntLiteral)?;
        let text = tok.text.replace('_', "");
        let val: i64 = if text.starts_with("16#") {
            i64::from_str_radix(&text[3..], 16)
        } else if text.starts_with("8#") {
            i64::from_str_radix(&text[2..], 8)
        } else if text.starts_with("2#") {
            i64::from_str_radix(&text[2..], 2)
        } else {
            text.parse()
        }
        .map_err(|_| ParseError {
            message: format!("invalid integer literal '{}'", tok.text),
            line: tok.line,
            col: tok.col,
        })?;
        Ok(if negative { -val } else { val })
    }

    // ── Statement parsing ───────────────────────────────────────

    /// Parses statements until one of the `terminators` is reached.
    fn parse_statement_list(&mut self, terminators: &[TokenType]) -> ParseResult<Vec<Statement>> {
        let mut stmts = Vec::new();
        while !terminators.contains(&self.current_kind()) && self.current_kind() != TokenType::Eof {
            stmts.push(self.parse_statement()?);
        }
        Ok(stmts)
    }

    /// Parses a single statement.
    fn parse_statement(&mut self) -> ParseResult<Statement> {
        // Handle bare semicolons
        if self.eat(TokenType::SemiColon) {
            return Ok(Statement::Empty);
        }

        match self.current_kind() {
            TokenType::If     => self.parse_if(),
            TokenType::For    => self.parse_for(),
            TokenType::While  => self.parse_while(),
            TokenType::Repeat => self.parse_repeat(),
            TokenType::Case   => self.parse_case(),
            TokenType::Exit   => {
                let span = self.span();
                self.advance();
                self.expect(TokenType::SemiColon)?;
                Ok(Statement::Exit { span })
            }
            TokenType::Return => {
                let span = self.span();
                self.advance();
                self.expect(TokenType::SemiColon)?;
                Ok(Statement::Return { span })
            }
            _ => self.parse_assignment_or_call(),
        }
    }

    /// Parses either an assignment (`x := expr;`) or a bare call
    /// (`MyFB(args);`).
    fn parse_assignment_or_call(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        let target = self.parse_expression()?;

        if self.eat(TokenType::Assignment) {
            let value = self.parse_expression()?;
            self.expect(TokenType::SemiColon)?;
            Ok(Statement::Assignment { target, value, span })
        } else {
            // Bare expression statement — should be a function call
            self.expect(TokenType::SemiColon)?;
            // Wrap the expression as a call statement if it's a function call,
            // otherwise just treat it as an assignment to itself (semantic
            // analysis will catch invalid bare expressions)
            match target {
                Expression::FunctionCall { name, args, span } => {
                    Ok(Statement::CallStatement { name, args, span })
                }
                _ => {
                    // Bare expression — not valid ST, but let semantic
                    // analysis report it rather than the parser.
                    Ok(Statement::Assignment {
                        target: target.clone(),
                        value: target,
                        span,
                    })
                }
            }
        }
    }

    /// `IF cond THEN stmts {ELSIF cond THEN stmts} [ELSE stmts] END_IF ;`
    fn parse_if(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.expect(TokenType::If)?;
        let condition = self.parse_expression()?;
        self.expect(TokenType::Then)?;

        let then_body = self.parse_statement_list(&[
            TokenType::Elsif, TokenType::Else, TokenType::EndIf,
        ])?;

        let mut elsif_branches = Vec::new();
        while self.eat(TokenType::Elsif) {
            let cond = self.parse_expression()?;
            self.expect(TokenType::Then)?;
            let body = self.parse_statement_list(&[
                TokenType::Elsif, TokenType::Else, TokenType::EndIf,
            ])?;
            elsif_branches.push((cond, body));
        }

        let else_body = if self.eat(TokenType::Else) {
            Some(self.parse_statement_list(&[TokenType::EndIf])?)
        } else {
            None
        };

        self.expect(TokenType::EndIf)?;
        self.expect(TokenType::SemiColon)?;

        Ok(Statement::If { condition, then_body, elsif_branches, else_body, span })
    }

    /// `FOR var := from TO to [BY step] DO stmts END_FOR ;`
    fn parse_for(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.expect(TokenType::For)?;
        let variable = self.expect(TokenType::Ident)?.text;
        self.expect(TokenType::Assignment)?;
        let from = self.parse_expression()?;
        self.expect(TokenType::To)?;
        let to = self.parse_expression()?;

        let by = if self.eat(TokenType::By) {
            Some(self.parse_expression()?)
        } else {
            None
        };

        self.expect(TokenType::Do)?;
        let body = self.parse_statement_list(&[TokenType::EndFor])?;
        self.expect(TokenType::EndFor)?;
        self.expect(TokenType::SemiColon)?;

        Ok(Statement::For { variable, from, to, by, body, span })
    }

    /// `WHILE cond DO stmts END_WHILE ;`
    fn parse_while(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.expect(TokenType::While)?;
        let condition = self.parse_expression()?;
        self.expect(TokenType::Do)?;
        let body = self.parse_statement_list(&[TokenType::EndWhile])?;
        self.expect(TokenType::EndWhile)?;
        self.expect(TokenType::SemiColon)?;
        Ok(Statement::While { condition, body, span })
    }

    /// `REPEAT stmts UNTIL cond END_REPEAT ;`
    fn parse_repeat(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.expect(TokenType::Repeat)?;
        let body = self.parse_statement_list(&[TokenType::Until])?;
        self.expect(TokenType::Until)?;
        let condition = self.parse_expression()?;
        self.expect(TokenType::EndRepeat)?;
        self.expect(TokenType::SemiColon)?;
        Ok(Statement::Repeat { body, condition, span })
    }

    /// `CASE selector OF {labels : stmts} [ELSE stmts] END_CASE ;`
    fn parse_case(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.expect(TokenType::Case)?;
        let selector = self.parse_expression()?;
        self.expect(TokenType::Of)?;

        let mut branches = Vec::new();
        while self.current_kind() != TokenType::Else
            && self.current_kind() != TokenType::EndCase
            && self.current_kind() != TokenType::Eof
        {
            branches.push(self.parse_case_branch()?);
        }

        let else_body = if self.eat(TokenType::Else) {
            Some(self.parse_statement_list(&[TokenType::EndCase])?)
        } else {
            None
        };

        self.expect(TokenType::EndCase)?;
        self.expect(TokenType::SemiColon)?;

        Ok(Statement::Case { selector, branches, else_body, span })
    }

    /// Parses a single case branch: `label {, label} : stmts`
    fn parse_case_branch(&mut self) -> ParseResult<CaseBranch> {
        let mut labels = Vec::new();

        loop {
            let expr = self.parse_expression()?;
            if self.eat(TokenType::DotDot) {
                let high = self.parse_expression()?;
                labels.push(CaseLabel::Range(expr, high));
            } else {
                labels.push(CaseLabel::Value(expr));
            }
            if !self.eat(TokenType::Comma) {
                break;
            }
        }

        self.expect(TokenType::Colon)?;

        // Parse statements until next label, ELSE, or END_CASE
        let mut body = Vec::new();
        loop {
            let k = self.current_kind();
            if k == TokenType::Else || k == TokenType::EndCase || k == TokenType::Eof {
                break;
            }
            // Peek: if we see int/ident followed by : or .., it's the next label
            if self.is_case_label_start() {
                break;
            }
            body.push(self.parse_statement()?);
        }

        Ok(CaseBranch { labels, body })
    }

    /// Heuristic: are we looking at the start of a new case label?
    fn is_case_label_start(&self) -> bool {
        // Look for patterns like `0:`, `1..5:`, `ident:`
        let k = self.current_kind();
        if k == TokenType::IntLiteral || k == TokenType::Ident || k == TokenType::Minus {
            // Scan ahead for ':' or '..'
            let mut lookahead = self.pos;
            while lookahead < self.tokens.len() {
                let tk = self.tokens[lookahead].kind;
                match tk {
                    TokenType::IntLiteral | TokenType::Ident | TokenType::Minus
                    | TokenType::DotDot | TokenType::Comma | TokenType::Plus => {
                        lookahead += 1;
                    }
                    TokenType::Colon => return true,
                    _ => return false,
                }
            }
        }
        false
    }

    // ── Expression parsing (precedence climbing) ────────────────
    //
    // IEC 61131-3 precedence (lowest → highest):
    //  1. OR
    //  2. XOR
    //  3. AND / &
    //  4. =  <>
    //  5. <  >  <=  >=
    //  6. +  -  (binary)
    //  7. *  /  MOD
    //  8. **  (right-associative)
    //  9. NOT  unary+  unary-
    // 10. primary (literals, identifiers, calls, subscripts, parens)

    /// Parses a full expression starting at the lowest precedence.
    pub fn parse_expression(&mut self) -> ParseResult<Expression> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_xor_expr()?;
        while self.current_kind() == TokenType::Or {
            let span = self.span();
            self.advance();
            let right = self.parse_xor_expr()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op: BinaryOperator::Or,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_xor_expr(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_and_expr()?;
        while self.current_kind() == TokenType::Xor {
            let span = self.span();
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op: BinaryOperator::Xor,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_equality_expr()?;
        while self.current_kind() == TokenType::And || self.current_kind() == TokenType::Ampersand {
            let span = self.span();
            self.advance();
            let right = self.parse_equality_expr()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op: BinaryOperator::And,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_equality_expr(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_comparison_expr()?;
        loop {
            let op = match self.current_kind() {
                TokenType::Equal    => BinaryOperator::Eq,
                TokenType::NotEqual => BinaryOperator::Neq,
                _ => break,
            };
            let span = self.span();
            self.advance();
            let right = self.parse_comparison_expr()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_comparison_expr(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_add_expr()?;
        loop {
            let op = match self.current_kind() {
                TokenType::Less      => BinaryOperator::Lt,
                TokenType::LessEq    => BinaryOperator::Le,
                TokenType::Greater   => BinaryOperator::Gt,
                TokenType::GreaterEq => BinaryOperator::Ge,
                _ => break,
            };
            let span = self.span();
            self.advance();
            let right = self.parse_add_expr()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_add_expr(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_mul_expr()?;
        loop {
            let op = match self.current_kind() {
                TokenType::Plus  => BinaryOperator::Add,
                TokenType::Minus => BinaryOperator::Sub,
                _ => break,
            };
            let span = self.span();
            self.advance();
            let right = self.parse_mul_expr()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_mul_expr(&mut self) -> ParseResult<Expression> {
        let mut left = self.parse_power_expr()?;
        loop {
            let op = match self.current_kind() {
                TokenType::Star  => BinaryOperator::Mul,
                TokenType::Slash => BinaryOperator::Div,
                TokenType::Mod   => BinaryOperator::Mod,
                _ => break,
            };
            let span = self.span();
            self.advance();
            let right = self.parse_power_expr()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    /// `**` is right-associative.
    fn parse_power_expr(&mut self) -> ParseResult<Expression> {
        let base = self.parse_unary_expr()?;
        if self.current_kind() == TokenType::Power {
            let span = self.span();
            self.advance();
            let exp = self.parse_power_expr()?; // right-recursive
            Ok(Expression::BinaryOp {
                left: Box::new(base),
                op: BinaryOperator::Power,
                right: Box::new(exp),
                span,
            })
        } else {
            Ok(base)
        }
    }

    fn parse_unary_expr(&mut self) -> ParseResult<Expression> {
        match self.current_kind() {
            TokenType::Minus => {
                let span = self.span();
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(Expression::UnaryOp {
                    op: UnaryOperator::Neg,
                    operand: Box::new(operand),
                    span,
                })
            }
            TokenType::Plus => {
                let span = self.span();
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(Expression::UnaryOp {
                    op: UnaryOperator::Pos,
                    operand: Box::new(operand),
                    span,
                })
            }
            TokenType::Not => {
                let span = self.span();
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(Expression::UnaryOp {
                    op: UnaryOperator::Not,
                    operand: Box::new(operand),
                    span,
                })
            }
            _ => self.parse_postfix_expr(),
        }
    }

    /// Postfix: array subscript `[i]`, member access `.field`,
    /// function call `(args)`.
    fn parse_postfix_expr(&mut self) -> ParseResult<Expression> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.current_kind() {
                TokenType::LBracket => {
                    let span = self.span();
                    self.advance(); // consume '['
                    let mut indices = Vec::new();
                    loop {
                        indices.push(self.parse_expression()?);
                        if !self.eat(TokenType::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenType::RBracket)?;
                    expr = Expression::ArrayAccess {
                        array: Box::new(expr),
                        indices,
                        span,
                    };
                }
                TokenType::Dot => {
                    let span = self.span();
                    self.advance(); // consume '.'
                    let member = self.expect(TokenType::Ident)?.text;
                    expr = Expression::MemberAccess {
                        object: Box::new(expr),
                        member,
                        span,
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    /// Primary expressions: literals, identifiers, function calls, parens.
    fn parse_primary(&mut self) -> ParseResult<Expression> {
        let span = self.span();
        let tok = self.current().clone();

        match tok.kind {
            TokenType::IntLiteral => {
                self.advance();
                let text = tok.text.replace('_', "");
                let value: i64 = if text.starts_with("16#") {
                    i64::from_str_radix(&text[3..], 16)
                } else if text.starts_with("8#") {
                    i64::from_str_radix(&text[2..], 8)
                } else if text.starts_with("2#") {
                    i64::from_str_radix(&text[2..], 2)
                } else {
                    text.parse()
                }
                .map_err(|_| self.error(format!("invalid integer '{}'", tok.text)))?;
                Ok(Expression::IntLiteral { value, span })
            }

            TokenType::RealLiteral => {
                self.advance();
                let text = tok.text.replace('_', "");
                let value: f64 = text.parse()
                    .map_err(|_| self.error(format!("invalid real '{}'", tok.text)))?;
                Ok(Expression::RealLiteral { value, span })
            }

            TokenType::BoolLiteral => {
                self.advance();
                let value = tok.text.to_ascii_uppercase() == "TRUE";
                Ok(Expression::BoolLiteral { value, span })
            }

            TokenType::StringLiteral => {
                self.advance();
                Ok(Expression::StringLiteral { value: tok.text, span })
            }

            TokenType::WStringLiteral => {
                self.advance();
                Ok(Expression::WStringLiteral { value: tok.text, span })
            }

            TokenType::TimeLiteral => {
                self.advance();
                Ok(Expression::TimeLiteral { text: tok.text, span })
            }
            TokenType::DateLiteral => {
                self.advance();
                Ok(Expression::DateLiteral { text: tok.text, span })
            }
            TokenType::TodLiteral => {
                self.advance();
                Ok(Expression::TodLiteral { text: tok.text, span })
            }
            TokenType::DtLiteral => {
                self.advance();
                Ok(Expression::DtLiteral { text: tok.text, span })
            }

            TokenType::Ident => {
                let name = tok.text.clone();
                self.advance();

                // Check for function call: ident '('
                if self.current_kind() == TokenType::LParen {
                    self.advance(); // consume '('
                    let args = self.parse_call_args()?;
                    self.expect(TokenType::RParen)?;
                    Ok(Expression::FunctionCall { name, args, span })
                } else {
                    Ok(Expression::Identifier { name, span })
                }
            }

            TokenType::LParen => {
                self.advance(); // consume '('
                let expr = self.parse_expression()?;
                self.expect(TokenType::RParen)?;
                Ok(expr)
            }

            _ => Err(self.error(format!(
                "expected expression, found {:?} '{}'",
                tok.kind, tok.text
            ))),
        }
    }

    /// Parses function/FB call arguments (positional, named `:=`, output `=>`).
    fn parse_call_args(&mut self) -> ParseResult<Vec<CallArg>> {
        let mut args = Vec::new();
        if self.current_kind() == TokenType::RParen {
            return Ok(args);
        }

        loop {
            // Check for named arg: ident := expr  or  ident => expr
            if self.current_kind() == TokenType::Ident {
                let saved_pos = self.pos;
                let name = self.advance().text.clone();

                if self.eat(TokenType::Assignment) {
                    let value = self.parse_expression()?;
                    args.push(CallArg::Named { name, value });
                } else if self.eat(TokenType::OutputAssign) {
                    let target = self.parse_expression()?;
                    args.push(CallArg::Output { name, target });
                } else {
                    // Not named — backtrack and parse as positional
                    self.pos = saved_pos;
                    let expr = self.parse_expression()?;
                    args.push(CallArg::Positional(expr));
                }
            } else {
                let expr = self.parse_expression()?;
                args.push(CallArg::Positional(expr));
            }

            if !self.eat(TokenType::Comma) {
                break;
            }
        }

        Ok(args)
    }
}

// ─── Unit Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> ParseResult<CompilationUnit> {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer);
        parser.parse()
    }

    fn parse_ok(src: &str) -> CompilationUnit {
        parse(src).unwrap_or_else(|e| panic!("Parse error: {}", e))
    }

    // ── Minimal programs ────────────────────────────────────────

    #[test]
    fn test_empty_program() {
        let unit = parse_ok("PROGRAM P END_PROGRAM");
        assert_eq!(unit.units.len(), 1);
        if let Pou::Program(p) = &unit.units[0] {
            assert_eq!(p.name, "P");
            assert!(p.var_blocks.is_empty());
            assert!(p.body.is_empty());
        } else {
            panic!("expected Program");
        }
    }

    #[test]
    fn test_program_with_var() {
        let unit = parse_ok("PROGRAM P VAR x : INT := 0; END_VAR END_PROGRAM");
        if let Pou::Program(p) = &unit.units[0] {
            assert_eq!(p.var_blocks.len(), 1);
            assert_eq!(p.var_blocks[0].declarations.len(), 1);
            assert_eq!(p.var_blocks[0].declarations[0].name, "x");
        } else {
            panic!("expected Program");
        }
    }

    #[test]
    fn test_multiple_vars() {
        let unit = parse_ok(
            "PROGRAM P VAR x : INT; y : REAL := 3.14; z : BOOL; END_VAR END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            assert_eq!(p.var_blocks[0].declarations.len(), 3);
        } else {
            panic!("expected Program");
        }
    }

    #[test]
    fn test_array_var() {
        let unit = parse_ok(
            "PROGRAM P VAR a : ARRAY[0..9] OF DINT; END_VAR END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            let ts = &p.var_blocks[0].declarations[0].type_spec;
            if let TypeSpec::Array { ranges, element_type } = ts {
                assert_eq!(ranges[0].low, 0);
                assert_eq!(ranges[0].high, 9);
                assert_eq!(**element_type, TypeSpec::Elementary(ElementaryType::Dint));
            } else {
                panic!("expected Array type");
            }
        } else {
            panic!("expected Program");
        }
    }

    // ── Assignments and expressions ─────────────────────────────

    #[test]
    fn test_simple_assignment() {
        let unit = parse_ok("PROGRAM P VAR x : INT; END_VAR x := 42; END_PROGRAM");
        if let Pou::Program(p) = &unit.units[0] {
            assert_eq!(p.body.len(), 1);
            if let Statement::Assignment { value, .. } = &p.body[0] {
                if let Expression::IntLiteral { value: v, .. } = value {
                    assert_eq!(*v, 42);
                } else {
                    panic!("expected IntLiteral");
                }
            } else {
                panic!("expected Assignment");
            }
        } else {
            panic!("expected Program");
        }
    }

    #[test]
    fn test_operator_precedence() {
        // 2 + 3 * 4 should parse as 2 + (3 * 4)
        let unit = parse_ok("PROGRAM P VAR x : INT; END_VAR x := 2 + 3 * 4; END_PROGRAM");
        if let Pou::Program(p) = &unit.units[0] {
            if let Statement::Assignment { value, .. } = &p.body[0] {
                if let Expression::BinaryOp { op, right, .. } = value {
                    assert_eq!(*op, BinaryOperator::Add);
                    // right should be 3*4
                    if let Expression::BinaryOp { op: inner_op, .. } = right.as_ref() {
                        assert_eq!(*inner_op, BinaryOperator::Mul);
                    } else {
                        panic!("expected BinaryOp for multiplication");
                    }
                } else {
                    panic!("expected BinaryOp for addition");
                }
            }
        }
    }

    #[test]
    fn test_boolean_expression() {
        let unit = parse_ok(
            "PROGRAM P VAR a : BOOL; b : BOOL; END_VAR a := b AND NOT a OR TRUE; END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            // Should parse as: (b AND (NOT a)) OR TRUE
            if let Statement::Assignment { value, .. } = &p.body[0] {
                if let Expression::BinaryOp { op, .. } = value {
                    assert_eq!(*op, BinaryOperator::Or); // OR is lowest
                } else {
                    panic!("expected BinaryOp");
                }
            }
        }
    }

    #[test]
    fn test_unary_minus() {
        let unit = parse_ok("PROGRAM P VAR x : INT; END_VAR x := -42; END_PROGRAM");
        if let Pou::Program(p) = &unit.units[0] {
            if let Statement::Assignment { value, .. } = &p.body[0] {
                if let Expression::UnaryOp { op, .. } = value {
                    assert_eq!(*op, UnaryOperator::Neg);
                } else {
                    panic!("expected UnaryOp");
                }
            }
        }
    }

    #[test]
    fn test_parenthesized_expression() {
        // (2 + 3) * 4 — parens override precedence
        let unit = parse_ok("PROGRAM P VAR x : INT; END_VAR x := (2 + 3) * 4; END_PROGRAM");
        if let Pou::Program(p) = &unit.units[0] {
            if let Statement::Assignment { value, .. } = &p.body[0] {
                if let Expression::BinaryOp { op, left, .. } = value {
                    assert_eq!(*op, BinaryOperator::Mul);
                    if let Expression::BinaryOp { op: inner, .. } = left.as_ref() {
                        assert_eq!(*inner, BinaryOperator::Add);
                    }
                }
            }
        }
    }

    // ── Control flow ────────────────────────────────────────────

    #[test]
    fn test_if_then() {
        let unit = parse_ok(
            "PROGRAM P VAR x : INT; END_VAR IF x > 0 THEN x := 0; END_IF; END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            if let Statement::If { then_body, .. } = &p.body[0] {
                assert_eq!(then_body.len(), 1);
            } else {
                panic!("expected If");
            }
        }
    }

    #[test]
    fn test_if_elsif_else() {
        let unit = parse_ok(
            "PROGRAM P VAR x : INT; END_VAR \
             IF x > 10 THEN x := 10; \
             ELSIF x > 5 THEN x := 5; \
             ELSIF x > 0 THEN x := 1; \
             ELSE x := 0; END_IF; END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            if let Statement::If { elsif_branches, else_body, .. } = &p.body[0] {
                assert_eq!(elsif_branches.len(), 2);
                assert!(else_body.is_some());
            }
        }
    }

    #[test]
    fn test_for_loop() {
        let unit = parse_ok(
            "PROGRAM P VAR i : INT; s : INT; END_VAR \
             FOR i := 0 TO 10 BY 2 DO s := s + i; END_FOR; END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            if let Statement::For { variable, by, body, .. } = &p.body[0] {
                assert_eq!(variable, "i");
                assert!(by.is_some());
                assert_eq!(body.len(), 1);
            }
        }
    }

    #[test]
    fn test_while_loop() {
        let unit = parse_ok(
            "PROGRAM P VAR x : INT; END_VAR \
             WHILE x > 0 DO x := x - 1; END_WHILE; END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            assert!(matches!(&p.body[0], Statement::While { .. }));
        }
    }

    #[test]
    fn test_repeat_loop() {
        let unit = parse_ok(
            "PROGRAM P VAR x : INT; END_VAR \
             REPEAT x := x + 1; UNTIL x >= 10 END_REPEAT; END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            assert!(matches!(&p.body[0], Statement::Repeat { .. }));
        }
    }

    #[test]
    fn test_case_statement() {
        let unit = parse_ok(
            "PROGRAM P VAR s : INT; x : REAL; END_VAR \
             CASE s OF \
                0: x := 0.0; \
                1..3: x := 1.0; \
                4, 5: x := 2.0; \
             ELSE x := -1.0; \
             END_CASE; END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            if let Statement::Case { branches, else_body, .. } = &p.body[0] {
                assert_eq!(branches.len(), 3);
                assert!(else_body.is_some());
            }
        }
    }

    // ── Function and function block ─────────────────────────────

    #[test]
    fn test_function_decl() {
        let unit = parse_ok(
            "FUNCTION Add : INT \
             VAR_INPUT a : INT; b : INT; END_VAR \
             Add := a + b; \
             END_FUNCTION"
        );
        if let Pou::Function(f) = &unit.units[0] {
            assert_eq!(f.name, "Add");
            assert_eq!(f.return_type, TypeSpec::Elementary(ElementaryType::Int));
            assert_eq!(f.var_blocks[0].qualifier, VarQualifier::VarInput);
        }
    }

    #[test]
    fn test_function_block_decl() {
        let unit = parse_ok(
            "FUNCTION_BLOCK Counter \
             VAR_INPUT enable : BOOL; END_VAR \
             VAR_OUTPUT count : INT; END_VAR \
             VAR internal : INT := 0; END_VAR \
             IF enable THEN internal := internal + 1; END_IF; \
             count := internal; \
             END_FUNCTION_BLOCK"
        );
        if let Pou::FunctionBlock(fb) = &unit.units[0] {
            assert_eq!(fb.name, "Counter");
            assert_eq!(fb.var_blocks.len(), 3);
            assert_eq!(fb.body.len(), 2);
        }
    }

    // ── Array access and member access ──────────────────────────

    #[test]
    fn test_array_access() {
        let unit = parse_ok(
            "PROGRAM P VAR a : ARRAY[0..9] OF INT; i : INT; END_VAR \
             a[i] := a[i] + 1; END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            if let Statement::Assignment { target, .. } = &p.body[0] {
                assert!(matches!(target, Expression::ArrayAccess { .. }));
            }
        }
    }

    #[test]
    fn test_member_access() {
        let unit = parse_ok(
            "PROGRAM P VAR x : INT; END_VAR x := myFB.output; END_PROGRAM"
        );
        if let Pou::Program(p) = &unit.units[0] {
            if let Statement::Assignment { value, .. } = &p.body[0] {
                if let Expression::MemberAccess { member, .. } = value {
                    assert_eq!(member, "output");
                }
            }
        }
    }

    // ── Error handling ──────────────────────────────────────────

    #[test]
    fn test_missing_semicolon() {
        let result = parse("PROGRAM P VAR x : INT; END_VAR x := 42 END_PROGRAM");
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_end_program() {
        let result = parse("PROGRAM P");
        assert!(result.is_err());
    }

    // ── Realistic program ───────────────────────────────────────

    #[test]
    fn test_conveyor_control() {
        let src = r#"
PROGRAM ConveyorControl
VAR
    speed : REAL := 0.0;
    running : BOOL := FALSE;
    count : INT := 0;
    limit : INT := 1000;
    sensor_vals : ARRAY[0..7] OF DINT;
END_VAR

IF running AND speed > 0.0 THEN
    count := count + 1;
    IF count >= limit THEN
        running := FALSE;
        speed := 0.0;
    ELSIF speed > 100.0 THEN
        speed := 100.0;
    END_IF;
END_IF;

FOR i := 0 TO 7 BY 1 DO
    sensor_vals[i] := sensor_vals[i] + 1;
END_FOR;

CASE count MOD 4 OF
    0: speed := 25.0;
    1..2: speed := 50.0;
    3: speed := 75.0;
END_CASE;

END_PROGRAM
"#;
        let unit = parse_ok(src);
        if let Pou::Program(p) = &unit.units[0] {
            assert_eq!(p.name, "ConveyorControl");
            assert_eq!(p.var_blocks[0].declarations.len(), 5);
            assert_eq!(p.body.len(), 3); // IF, FOR, CASE
        }
    }
}