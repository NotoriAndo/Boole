//! Deterministic resource meter for a deliberately restricted Rust-shaped
//! tuple-projection answer language.
//!
//! The meter is deliberately standalone and non-activatable. It does not
//! invoke a compiler, inspect host telemetry, or modify the V1 checker. A
//! caller supplies every bound, and every counter uses checked arithmetic.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

const ABSOLUTE_MAX_SYNTAX_DEPTH: u64 = 256;
const RESOURCE_USE_DOMAIN: &[u8] = b"boole.native-rust-meter.resource-use.v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterLimits {
    pub max_source_bytes: u64,
    pub max_tokens: u64,
    pub max_ast_nodes: u64,
    pub max_ast_depth: u64,
    pub max_operations: u64,
    pub max_fuel: u64,
    pub max_prefix_items: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeterError {
    InvalidSyntax,
    ForbiddenConstruct(&'static str),
    BudgetExceeded(&'static str),
    CounterOverflow,
    InvalidProgram(&'static str),
}

impl fmt::Display for MeterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for MeterError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TupleField {
    Signed(i64),
    Unsigned(u64),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TupleItem {
    fields: Vec<TupleField>,
}

impl TupleItem {
    pub fn new(fields: Vec<TupleField>) -> Self {
        Self { fields }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceUse {
    source_bytes: u64,
    tokens: u64,
    ast_nodes: u64,
    max_ast_depth: u64,
    operations: u64,
    fuel: u64,
    prefix_items: u64,
}

impl ResourceUse {
    pub fn source_bytes(self) -> u64 {
        self.source_bytes
    }

    pub fn tokens(self) -> u64 {
        self.tokens
    }

    pub fn ast_nodes(self) -> u64 {
        self.ast_nodes
    }

    pub fn max_ast_depth(self) -> u64 {
        self.max_ast_depth
    }

    pub fn operations(self) -> u64 {
        self.operations
    }

    pub fn fuel(self) -> u64 {
        self.fuel
    }

    pub fn prefix_items(self) -> u64 {
        self.prefix_items
    }

    /// Stable bytes for hashing/receipt binding. Host time, memory and process
    /// observations are intentionally absent.
    pub fn canonical_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(RESOURCE_USE_DOMAIN.len() + 7 * 8);
        bytes.extend_from_slice(RESOURCE_USE_DOMAIN);
        for value in [
            self.source_bytes,
            self.tokens,
            self.ast_nodes,
            self.max_ast_depth,
            self.operations,
            self.fuel,
            self.prefix_items,
        ] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, MeterError> {
        if bytes.len() != RESOURCE_USE_DOMAIN.len() + 7 * 8
            || !bytes.starts_with(RESOURCE_USE_DOMAIN)
        {
            return Err(MeterError::InvalidSyntax);
        }
        let mut values = [0u64; 7];
        for (slot, chunk) in values
            .iter_mut()
            .zip(bytes[RESOURCE_USE_DOMAIN.len()..].chunks_exact(8))
        {
            *slot = u64::from_be_bytes(chunk.try_into().map_err(|_| MeterError::InvalidSyntax)?);
        }
        Ok(Self {
            source_bytes: values[0],
            tokens: values[1],
            ast_nodes: values[2],
            max_ast_depth: values[3],
            operations: values[4],
            fuel: values[5],
            prefix_items: values[6],
        })
    }

    /// Checked aggregation for callers that combine independent deterministic
    /// evaluations. Depth is a maximum; all other fields are sums.
    pub fn try_add(self, other: Self) -> Result<Self, MeterError> {
        Ok(Self {
            source_bytes: checked_add(self.source_bytes, other.source_bytes)?,
            tokens: checked_add(self.tokens, other.tokens)?,
            ast_nodes: checked_add(self.ast_nodes, other.ast_nodes)?,
            max_ast_depth: self.max_ast_depth.max(other.max_ast_depth),
            operations: checked_add(self.operations, other.operations)?,
            fuel: checked_add(self.fuel, other.fuel)?,
            prefix_items: checked_add(self.prefix_items, other.prefix_items)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct MeteredProgram {
    ast: Program,
    static_use: ResourceUse,
}

impl MeteredProgram {
    pub fn static_resource_use(&self) -> ResourceUse {
        self.static_use
    }

    pub fn required_prefix_items(&self, item_count: u64) -> Result<u64, MeterError> {
        if !self
            .ast
            .statements
            .iter()
            .any(|statement| matches!(statement, Statement::ForItems { .. }))
        {
            return Ok(0);
        }
        let successor = item_count
            .checked_add(1)
            .ok_or(MeterError::CounterOverflow)?;
        let (left, right) = if item_count.is_multiple_of(2) {
            (item_count / 2, successor)
        } else {
            (item_count, successor / 2)
        };
        left.checked_mul(right).ok_or(MeterError::CounterOverflow)
    }

    /// Evaluate the same program on prefixes `1..=items.len()`, matching the
    /// tuple-family hidden harness. The item-visit counter is therefore
    /// `1 + ... + n` for a program containing one `for ... in items` loop.
    pub fn evaluate_hidden_prefixes(
        &self,
        items: &[TupleItem],
        limits: MeterLimits,
    ) -> Result<Evaluation, MeterError> {
        validate_program_types(&self.ast, items)?;
        let item_count = u64::try_from(items.len()).map_err(|_| MeterError::CounterOverflow)?;
        let required_prefix_items = self.required_prefix_items(item_count)?;
        ensure_limit(
            required_prefix_items,
            limits.max_prefix_items,
            "prefix_items",
        )?;
        let mut runtime = Runtime::new(self.static_use, limits);
        let mut outputs = Vec::with_capacity(items.len());
        for end in 1..=items.len() {
            let mut environment = Environment::default();
            let output = runtime.execute_program(&self.ast, &items[..end], &mut environment)?;
            outputs.push(output);
        }
        Ok(Evaluation {
            outputs,
            use_: runtime.use_,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    outputs: Vec<i64>,
    use_: ResourceUse,
}

impl Evaluation {
    pub fn outputs(&self) -> &[i64] {
        &self.outputs
    }

    pub fn resource_use(&self) -> ResourceUse {
        self.use_
    }
}

pub fn parse(source: &str, limits: MeterLimits) -> Result<MeteredProgram, MeterError> {
    let source_bytes = u64::try_from(source.len()).map_err(|_| MeterError::CounterOverflow)?;
    ensure_limit(source_bytes, limits.max_source_bytes, "source_bytes")?;
    let tokens = lex(source, limits.max_tokens)?;
    let token_count = u64::try_from(tokens.len()).map_err(|_| MeterError::CounterOverflow)?;
    let mut parser = Parser::new(tokens, limits.max_ast_depth);
    let ast = parser.parse_program()?;
    let (ast_nodes, ast_depth) = ast_statistics(&ast)?;
    let max_ast_depth = ast_depth.max(parser.max_syntax_depth);
    ensure_limit(ast_nodes, limits.max_ast_nodes, "ast_nodes")?;
    ensure_limit(max_ast_depth, limits.max_ast_depth, "ast_depth")?;

    Ok(MeteredProgram {
        ast,
        static_use: ResourceUse {
            source_bytes,
            tokens: token_count,
            ast_nodes,
            max_ast_depth,
            ..ResourceUse::default()
        },
    })
}

fn checked_add(left: u64, right: u64) -> Result<u64, MeterError> {
    left.checked_add(right).ok_or(MeterError::CounterOverflow)
}

fn ensure_limit(value: u64, limit: u64, name: &'static str) -> Result<(), MeterError> {
    if value > limit {
        Err(MeterError::BudgetExceeded(name))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Let,
    Mut,
    For,
    In,
    If,
    Else,
    As,
    I64,
    True,
    False,
    Ident(String),
    Int(u64),
    LBrace,
    RBrace,
    LParen,
    RParen,
    Dot,
    Colon,
    Semicolon,
    Equal,
    Minus,
}

fn lex(source: &str, max_tokens: u64) -> Result<Vec<Token>, MeterError> {
    let bytes = source.as_bytes();
    let mut offset = 0usize;
    let mut tokens = Vec::new();
    while offset < bytes.len() {
        let byte = bytes[offset];
        if byte.is_ascii_whitespace() {
            offset += 1;
            continue;
        }
        if byte == b'/'
            && bytes
                .get(offset + 1)
                .is_some_and(|next| *next == b'/' || *next == b'*')
        {
            return Err(MeterError::ForbiddenConstruct("comment"));
        }
        if starts_rust_literal(bytes, offset) {
            return Err(MeterError::ForbiddenConstruct("string_or_char"));
        }
        if byte == b'!' {
            return Err(MeterError::ForbiddenConstruct("macro"));
        }
        if byte == b'|' {
            return Err(MeterError::ForbiddenConstruct("closure"));
        }
        if byte == b'<' || byte == b'>' {
            return Err(MeterError::ForbiddenConstruct("generic"));
        }

        let token = if byte.is_ascii_digit() {
            let start = offset;
            while bytes.get(offset).is_some_and(u8::is_ascii_digit) {
                offset += 1;
            }
            let number = source[start..offset]
                .parse::<u64>()
                .map_err(|_| MeterError::InvalidSyntax)?;
            Token::Int(number)
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = offset;
            while bytes
                .get(offset)
                .is_some_and(|next| next.is_ascii_alphanumeric() || *next == b'_')
            {
                offset += 1;
            }
            match &source[start..offset] {
                "let" => Token::Let,
                "mut" => Token::Mut,
                "for" => Token::For,
                "in" => Token::In,
                "if" => Token::If,
                "else" => Token::Else,
                "as" => Token::As,
                "i64" => Token::I64,
                "true" => Token::True,
                "false" => Token::False,
                "loop" | "while" => return Err(MeterError::ForbiddenConstruct("loop")),
                "fn" | "struct" | "enum" | "impl" | "trait" | "mod" | "use" | "static" | "type"
                | "pub" => return Err(MeterError::ForbiddenConstruct("local_item")),
                "const" => return Err(MeterError::ForbiddenConstruct("const")),
                "unsafe" | "extern" | "async" | "await" | "return" | "break" | "continue"
                | "match" | "move" | "dyn" | "where" => {
                    return Err(MeterError::ForbiddenConstruct("unsupported_keyword"))
                }
                ident => Token::Ident(ident.to_owned()),
            }
        } else {
            offset += 1;
            match byte {
                b'{' => Token::LBrace,
                b'}' => Token::RBrace,
                b'(' => Token::LParen,
                b')' => Token::RParen,
                b'.' => Token::Dot,
                b':' => Token::Colon,
                b';' => Token::Semicolon,
                b'=' => Token::Equal,
                b'-' => Token::Minus,
                _ => return Err(MeterError::InvalidSyntax),
            }
        };
        tokens.push(token);
        let count = u64::try_from(tokens.len()).map_err(|_| MeterError::CounterOverflow)?;
        ensure_limit(count, max_tokens, "tokens")?;
    }
    Ok(tokens)
}

fn starts_rust_literal(bytes: &[u8], offset: usize) -> bool {
    let Some(first) = bytes.get(offset).copied() else {
        return false;
    };
    if first == b'\'' || first == b'"' {
        return true;
    }
    if first == b'b' && matches!(bytes.get(offset + 1), Some(b'\'' | b'"')) {
        return true;
    }

    let mut cursor = offset;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return false;
    }
    cursor += 1;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    bytes.get(cursor) == Some(&b'"')
}

#[derive(Debug, Clone)]
struct Program {
    statements: Vec<Statement>,
    tail: Expression,
}

#[derive(Debug, Clone)]
enum Statement {
    Let {
        name: String,
        mutable: bool,
        declared_i64: bool,
        expression: Expression,
    },
    Assign {
        name: String,
        expression: Expression,
    },
    ForItems {
        binding: String,
        body: Vec<Statement>,
    },
}

#[derive(Debug, Clone)]
enum Expression {
    I64(i64),
    Bool(bool),
    Variable(String),
    Field(Box<Expression>, u8),
    CastI64(Box<Expression>),
    Wrapping {
        operation: WrappingOperation,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    If {
        condition: Box<Expression>,
        then_expression: Box<Expression>,
        else_expression: Box<Expression>,
    },
}

#[derive(Debug, Clone, Copy)]
enum WrappingOperation {
    Add,
    Sub,
    Mul,
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    depth_limit: u64,
    max_syntax_depth: u64,
    top_level_for_seen: bool,
}

impl Parser {
    fn new(tokens: Vec<Token>, depth_limit: u64) -> Self {
        Self {
            tokens,
            cursor: 0,
            depth_limit,
            max_syntax_depth: 1,
            top_level_for_seen: false,
        }
    }

    fn parse_program(&mut self) -> Result<Program, MeterError> {
        if self.tokens.is_empty() {
            return Err(MeterError::InvalidSyntax);
        }
        let mut statements = Vec::new();
        while self.starts_statement() {
            statements.push(self.parse_statement(false, 2)?);
        }
        let tail = self.parse_expression(2)?;
        if self.cursor != self.tokens.len() {
            return Err(MeterError::ForbiddenConstruct("trailing_syntax"));
        }
        Ok(Program { statements, tail })
    }

    fn starts_statement(&self) -> bool {
        matches!(self.peek(), Some(Token::Let | Token::For))
            || matches!(
                (self.peek(), self.tokens.get(self.cursor + 1)),
                (Some(Token::Ident(_)), Some(Token::Equal))
            )
    }

    fn parse_statement(&mut self, inside_loop: bool, depth: u64) -> Result<Statement, MeterError> {
        self.observe_depth(depth)?;
        match self.peek() {
            Some(Token::Let) => self.parse_let(depth),
            Some(Token::For) if inside_loop => Err(MeterError::ForbiddenConstruct("nested_loop")),
            Some(Token::For) if self.top_level_for_seen => {
                Err(MeterError::ForbiddenConstruct("multiple_loop"))
            }
            Some(Token::For) => {
                self.top_level_for_seen = true;
                self.parse_for(depth)
            }
            Some(Token::Ident(_)) => self.parse_assignment(depth),
            _ => Err(MeterError::InvalidSyntax),
        }
    }

    fn parse_let(&mut self, depth: u64) -> Result<Statement, MeterError> {
        self.take();
        let mutable = self.take_if(&Token::Mut);
        let name = self.take_ident()?;
        if name == "items" {
            return Err(MeterError::InvalidProgram("reserved_binding"));
        }
        let declared_i64 = self.take_if(&Token::Colon);
        if declared_i64 {
            self.expect(&Token::I64)?;
        }
        self.expect(&Token::Equal)?;
        let expression = self.parse_expression(checked_add(depth, 1)?)?;
        self.expect(&Token::Semicolon)?;
        Ok(Statement::Let {
            name,
            mutable,
            declared_i64,
            expression,
        })
    }

    fn parse_assignment(&mut self, depth: u64) -> Result<Statement, MeterError> {
        let name = self.take_ident()?;
        self.expect(&Token::Equal)?;
        let expression = self.parse_expression(checked_add(depth, 1)?)?;
        self.expect(&Token::Semicolon)?;
        Ok(Statement::Assign { name, expression })
    }

    fn parse_for(&mut self, depth: u64) -> Result<Statement, MeterError> {
        self.take();
        let binding = self.take_ident()?;
        self.expect(&Token::In)?;
        match self.take() {
            Some(Token::Ident(name)) if name == "items" => {}
            Some(Token::LParen) => return Err(MeterError::ForbiddenConstruct("arbitrary_call")),
            _ => return Err(MeterError::InvalidSyntax),
        }
        self.expect(&Token::LBrace)?;
        let body_depth = checked_add(depth, 1)?;
        self.observe_depth(body_depth)?;
        let mut body = Vec::new();
        while !matches!(self.peek(), Some(Token::RBrace)) {
            if self.peek().is_none() {
                return Err(MeterError::InvalidSyntax);
            }
            if !self.starts_statement() {
                return Err(MeterError::InvalidSyntax);
            }
            body.push(self.parse_statement(true, body_depth)?);
        }
        self.expect(&Token::RBrace)?;
        Ok(Statement::ForItems { binding, body })
    }

    fn parse_expression(&mut self, depth: u64) -> Result<Expression, MeterError> {
        self.observe_depth(depth)?;
        let mut expression = match self.take() {
            Some(Token::Int(number)) => {
                let value = i64::try_from(number).map_err(|_| MeterError::InvalidSyntax)?;
                Expression::I64(value)
            }
            Some(Token::Minus) => match self.take() {
                Some(Token::Int(number)) if number == (1u64 << 63) => Expression::I64(i64::MIN),
                Some(Token::Int(number)) => {
                    let value = i64::try_from(number).map_err(|_| MeterError::InvalidSyntax)?;
                    Expression::I64(-value)
                }
                _ => return Err(MeterError::InvalidSyntax),
            },
            Some(Token::True) => Expression::Bool(true),
            Some(Token::False) => Expression::Bool(false),
            Some(Token::Ident(name)) => {
                if matches!(self.peek(), Some(Token::LParen)) {
                    return Err(MeterError::ForbiddenConstruct(if name == "acfr_solve" {
                        "recursion"
                    } else {
                        "arbitrary_call"
                    }));
                }
                Expression::Variable(name)
            }
            Some(Token::LParen) => {
                let nested = self.parse_expression(checked_add(depth, 1)?)?;
                self.expect(&Token::RParen)?;
                nested
            }
            Some(Token::If) => self.parse_if(depth)?,
            Some(Token::For) => return Err(MeterError::ForbiddenConstruct("nested_loop")),
            _ => return Err(MeterError::InvalidSyntax),
        };

        loop {
            if self.take_if(&Token::As) {
                self.expect(&Token::I64)?;
                expression = Expression::CastI64(Box::new(expression));
                continue;
            }
            if !self.take_if(&Token::Dot) {
                break;
            }
            match self.take() {
                Some(Token::Int(index)) => {
                    let index = u8::try_from(index).map_err(|_| MeterError::InvalidSyntax)?;
                    if index >= 8 {
                        return Err(MeterError::InvalidSyntax);
                    }
                    expression = Expression::Field(Box::new(expression), index);
                }
                Some(Token::Ident(method)) => {
                    let operation = match method.as_str() {
                        "wrapping_add" => WrappingOperation::Add,
                        "wrapping_sub" => WrappingOperation::Sub,
                        "wrapping_mul" => WrappingOperation::Mul,
                        _ => return Err(MeterError::ForbiddenConstruct("arbitrary_call")),
                    };
                    self.expect(&Token::LParen)?;
                    let right = self.parse_expression(checked_add(depth, 1)?)?;
                    self.expect(&Token::RParen)?;
                    expression = Expression::Wrapping {
                        operation,
                        left: Box::new(expression),
                        right: Box::new(right),
                    };
                }
                _ => return Err(MeterError::InvalidSyntax),
            }
        }
        Ok(expression)
    }

    fn parse_if(&mut self, depth: u64) -> Result<Expression, MeterError> {
        let nested_depth = checked_add(depth, 1)?;
        let condition = self.parse_expression(nested_depth)?;
        self.expect(&Token::LBrace)?;
        let then_expression = self.parse_expression(nested_depth)?;
        self.expect(&Token::RBrace)?;
        self.expect(&Token::Else)?;
        self.expect(&Token::LBrace)?;
        let else_expression = self.parse_expression(nested_depth)?;
        self.expect(&Token::RBrace)?;
        Ok(Expression::If {
            condition: Box::new(condition),
            then_expression: Box::new(then_expression),
            else_expression: Box::new(else_expression),
        })
    }

    fn observe_depth(&mut self, depth: u64) -> Result<(), MeterError> {
        self.max_syntax_depth = self.max_syntax_depth.max(depth);
        ensure_limit(
            depth,
            self.depth_limit.min(ABSOLUTE_MAX_SYNTAX_DEPTH),
            "ast_depth",
        )
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    fn take(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.cursor).cloned();
        self.cursor = self.cursor.saturating_add(usize::from(token.is_some()));
        token
    }

    fn take_if(&mut self, expected: &Token) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<(), MeterError> {
        if self.take_if(expected) {
            Ok(())
        } else {
            Err(MeterError::InvalidSyntax)
        }
    }

    fn take_ident(&mut self) -> Result<String, MeterError> {
        match self.take() {
            Some(Token::Ident(name)) => Ok(name),
            _ => Err(MeterError::InvalidSyntax),
        }
    }
}

fn ast_statistics(program: &Program) -> Result<(u64, u64), MeterError> {
    fn visit_expression(
        value: &Expression,
        depth: u64,
        use_: &mut (u64, u64),
    ) -> Result<(), MeterError> {
        ensure_limit(depth, ABSOLUTE_MAX_SYNTAX_DEPTH, "ast_depth")?;
        use_.0 = checked_add(use_.0, 1)?;
        use_.1 = use_.1.max(depth);
        match value {
            Expression::Field(base, _) | Expression::CastI64(base) => {
                visit_expression(base, checked_add(depth, 1)?, use_)?;
            }
            Expression::Wrapping { left, right, .. } => {
                visit_expression(left, checked_add(depth, 1)?, use_)?;
                visit_expression(right, checked_add(depth, 1)?, use_)?;
            }
            Expression::If {
                condition,
                then_expression,
                else_expression,
            } => {
                visit_expression(condition, checked_add(depth, 1)?, use_)?;
                visit_expression(then_expression, checked_add(depth, 1)?, use_)?;
                visit_expression(else_expression, checked_add(depth, 1)?, use_)?;
            }
            Expression::I64(_) | Expression::Bool(_) | Expression::Variable(_) => {}
        }
        Ok(())
    }

    fn visit_statement(
        value: &Statement,
        depth: u64,
        use_: &mut (u64, u64),
    ) -> Result<(), MeterError> {
        ensure_limit(depth, ABSOLUTE_MAX_SYNTAX_DEPTH, "ast_depth")?;
        use_.0 = checked_add(use_.0, 1)?;
        use_.1 = use_.1.max(depth);
        match value {
            Statement::Let {
                expression: value, ..
            }
            | Statement::Assign {
                expression: value, ..
            } => {
                visit_expression(value, checked_add(depth, 1)?, use_)?;
            }
            Statement::ForItems { body, .. } => {
                for nested in body {
                    visit_statement(nested, checked_add(depth, 1)?, use_)?;
                }
            }
        }
        Ok(())
    }

    let mut use_ = (1, 1);
    for statement_ in &program.statements {
        visit_statement(statement_, 2, &mut use_)?;
    }
    visit_expression(&program.tail, 2, &mut use_)?;
    Ok(use_)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StaticType {
    I64,
    NumericField,
    Bool,
    Tuple(Vec<ValueKind>),
}

#[derive(Debug, Clone)]
struct TypeBinding {
    kind: StaticType,
    mutable: bool,
}

#[derive(Debug, Default)]
struct TypeEnvironment {
    scopes: Vec<BTreeMap<String, TypeBinding>>,
}

impl TypeEnvironment {
    fn ensure_root(&mut self) {
        if self.scopes.is_empty() {
            self.scopes.push(BTreeMap::new());
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: String, kind: StaticType, mutable: bool) -> Result<(), MeterError> {
        self.ensure_root();
        let scope = self.scopes.last_mut().expect("root exists");
        if scope.contains_key(&name) {
            return Err(MeterError::InvalidProgram("duplicate_binding"));
        }
        scope.insert(name, TypeBinding { kind, mutable });
        Ok(())
    }

    fn lookup(&self, name: &str) -> Result<StaticType, MeterError> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .map(|binding| binding.kind.clone())
            .ok_or(MeterError::InvalidProgram("unknown_binding"))
    }

    fn assignment_target(&self, name: &str) -> Result<&TypeBinding, MeterError> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .ok_or(MeterError::InvalidProgram("unknown_binding"))
    }
}

fn validate_program_types(program: &Program, items: &[TupleItem]) -> Result<(), MeterError> {
    fn expression_kind(
        expression: &Expression,
        environment: &TypeEnvironment,
    ) -> Result<StaticType, MeterError> {
        match expression {
            Expression::I64(_) => Ok(StaticType::I64),
            Expression::Bool(_) => Ok(StaticType::Bool),
            Expression::Variable(name) => environment.lookup(name),
            Expression::Field(base, index) => {
                let StaticType::Tuple(fields) = expression_kind(base, environment)? else {
                    return Err(MeterError::InvalidProgram("field_on_non_tuple"));
                };
                match fields.get(usize::from(*index)) {
                    Some(ValueKind::I64) => Ok(StaticType::I64),
                    Some(ValueKind::NumericField) => Ok(StaticType::NumericField),
                    Some(ValueKind::Bool) => Ok(StaticType::Bool),
                    Some(ValueKind::Tuple) => Err(MeterError::InvalidProgram("nested_tuple_field")),
                    None => Err(MeterError::InvalidProgram("field_out_of_bounds")),
                }
            }
            Expression::CastI64(inner) => {
                if !matches!(
                    expression_kind(inner, environment)?,
                    StaticType::I64 | StaticType::NumericField
                ) {
                    return Err(MeterError::InvalidProgram("invalid_i64_cast"));
                }
                Ok(StaticType::I64)
            }
            Expression::Wrapping { left, right, .. } => {
                if expression_kind(left, environment)? != StaticType::I64
                    || expression_kind(right, environment)? != StaticType::I64
                {
                    return Err(MeterError::InvalidProgram("non_i64_wrapping_operand"));
                }
                Ok(StaticType::I64)
            }
            Expression::If {
                condition,
                then_expression,
                else_expression,
            } => {
                if expression_kind(condition, environment)? != StaticType::Bool {
                    return Err(MeterError::InvalidProgram("non_bool_condition"));
                }
                let then_kind = expression_kind(then_expression, environment)?;
                let else_kind = expression_kind(else_expression, environment)?;
                if then_kind != else_kind {
                    return Err(MeterError::InvalidProgram("if_branch_type_mismatch"));
                }
                Ok(then_kind)
            }
        }
    }

    fn statement_types(
        statement: &Statement,
        item_schema: &[ValueKind],
        environment: &mut TypeEnvironment,
    ) -> Result<(), MeterError> {
        match statement {
            Statement::Let {
                name,
                mutable,
                declared_i64,
                expression,
            } => {
                let kind = expression_kind(expression, environment)?;
                if *declared_i64 && kind != StaticType::I64 {
                    return Err(MeterError::InvalidProgram("declared_i64_type_mismatch"));
                }
                if kind == StaticType::NumericField {
                    return Err(MeterError::InvalidProgram("numeric_field_requires_cast"));
                }
                environment.declare(name.clone(), kind, *mutable)
            }
            Statement::Assign { name, expression } => {
                let kind = expression_kind(expression, environment)?;
                let target = environment.assignment_target(name)?;
                if !target.mutable {
                    return Err(MeterError::InvalidProgram("immutable_assignment"));
                }
                if target.kind != kind {
                    return Err(MeterError::InvalidProgram("assignment_type_mismatch"));
                }
                Ok(())
            }
            Statement::ForItems { binding, body } => {
                environment.push_scope();
                let result = (|| {
                    environment.declare(
                        binding.clone(),
                        StaticType::Tuple(item_schema.to_vec()),
                        false,
                    )?;
                    for nested in body {
                        statement_types(nested, item_schema, environment)?;
                    }
                    Ok(())
                })();
                environment.pop_scope();
                result
            }
        }
    }

    let has_loop = program
        .statements
        .iter()
        .any(|statement| matches!(statement, Statement::ForItems { .. }));
    let item_schema = if has_loop {
        let first = items
            .first()
            .ok_or(MeterError::InvalidProgram("empty_items"))?;
        if !(1..=8).contains(&first.fields.len()) {
            return Err(MeterError::InvalidProgram("tuple_schema_mismatch"));
        }
        let schema = first
            .fields
            .iter()
            .map(|field| match field {
                TupleField::Signed(_) | TupleField::Unsigned(_) => ValueKind::NumericField,
                TupleField::Bool(_) => ValueKind::Bool,
            })
            .collect::<Vec<_>>();
        if items.iter().skip(1).any(|item| {
            item.fields.len() != schema.len()
                || item.fields.iter().zip(&schema).any(|(field, expected)| {
                    let actual = match field {
                        TupleField::Signed(_) | TupleField::Unsigned(_) => ValueKind::NumericField,
                        TupleField::Bool(_) => ValueKind::Bool,
                    };
                    actual != *expected
                })
        }) {
            return Err(MeterError::InvalidProgram("tuple_schema_mismatch"));
        }
        schema
    } else {
        Vec::new()
    };

    let mut environment = TypeEnvironment::default();
    environment.ensure_root();
    for statement in &program.statements {
        statement_types(statement, &item_schema, &mut environment)?;
    }
    if expression_kind(&program.tail, &environment)? != StaticType::I64 {
        return Err(MeterError::InvalidProgram("non_i64_result"));
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum Value {
    I64(i64),
    NumericField(i64),
    Bool(bool),
    Tuple(TupleItem),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    I64,
    NumericField,
    Bool,
    Tuple,
}

impl Value {
    fn kind(&self) -> ValueKind {
        match self {
            Self::I64(_) => ValueKind::I64,
            Self::NumericField(_) => ValueKind::NumericField,
            Self::Bool(_) => ValueKind::Bool,
            Self::Tuple(_) => ValueKind::Tuple,
        }
    }
}

#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    kind: ValueKind,
    mutable: bool,
}

#[derive(Debug, Default)]
struct Environment {
    scopes: Vec<BTreeMap<String, Binding>>,
}

impl Environment {
    fn ensure_root(&mut self) {
        if self.scopes.is_empty() {
            self.scopes.push(BTreeMap::new());
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(
        &mut self,
        name: String,
        value: Value,
        mutable: bool,
        required_kind: Option<ValueKind>,
    ) -> Result<(), MeterError> {
        self.ensure_root();
        let kind = value.kind();
        if required_kind.is_some_and(|required| required != kind) {
            return Err(MeterError::InvalidProgram("declared_i64_type_mismatch"));
        }
        let scope = self.scopes.last_mut().expect("root exists");
        if scope.contains_key(&name) {
            return Err(MeterError::InvalidProgram("duplicate_binding"));
        }
        scope.insert(
            name,
            Binding {
                value,
                kind,
                mutable,
            },
        );
        Ok(())
    }

    fn lookup(&self, name: &str) -> Result<Value, MeterError> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .map(|binding| binding.value.clone())
            .ok_or(MeterError::InvalidProgram("unknown_binding"))
    }

    fn assign(&mut self, name: &str, value: Value) -> Result<(), MeterError> {
        let binding = self
            .scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
            .ok_or(MeterError::InvalidProgram("unknown_binding"))?;
        if !binding.mutable {
            return Err(MeterError::InvalidProgram("immutable_assignment"));
        }
        if binding.kind != value.kind() {
            return Err(MeterError::InvalidProgram("assignment_type_mismatch"));
        }
        binding.value = value;
        Ok(())
    }
}

struct Runtime {
    use_: ResourceUse,
    limits: MeterLimits,
}

impl Runtime {
    fn new(static_use: ResourceUse, limits: MeterLimits) -> Self {
        Self {
            use_: static_use,
            limits,
        }
    }

    fn execute_program(
        &mut self,
        program: &Program,
        items: &[TupleItem],
        environment: &mut Environment,
    ) -> Result<i64, MeterError> {
        environment.ensure_root();
        self.tick_fuel()?;
        for statement in &program.statements {
            self.execute_statement(statement, items, environment)?;
        }
        match self.evaluate_expression(&program.tail, environment)? {
            Value::I64(value) => Ok(value),
            _ => Err(MeterError::InvalidProgram("non_i64_result")),
        }
    }

    fn execute_statement(
        &mut self,
        statement: &Statement,
        items: &[TupleItem],
        environment: &mut Environment,
    ) -> Result<(), MeterError> {
        self.tick_fuel()?;
        match statement {
            Statement::Let {
                name,
                mutable,
                declared_i64,
                expression,
            } => {
                let value = self.evaluate_expression(expression, environment)?;
                environment.declare(
                    name.clone(),
                    value,
                    *mutable,
                    declared_i64.then_some(ValueKind::I64),
                )?;
                self.tick_operation()
            }
            Statement::Assign { name, expression } => {
                let value = self.evaluate_expression(expression, environment)?;
                environment.assign(name, value)?;
                self.tick_operation()
            }
            Statement::ForItems { binding, body } => {
                for item in items {
                    self.use_.prefix_items = checked_add(self.use_.prefix_items, 1)?;
                    ensure_limit(
                        self.use_.prefix_items,
                        self.limits.max_prefix_items,
                        "prefix_items",
                    )?;
                    self.tick_fuel()?;
                    environment.push_scope();
                    let result = (|| {
                        environment.declare(
                            binding.clone(),
                            Value::Tuple(item.clone()),
                            false,
                            Some(ValueKind::Tuple),
                        )?;
                        for nested in body {
                            self.execute_statement(nested, items, environment)?;
                        }
                        Ok(())
                    })();
                    environment.pop_scope();
                    result?;
                }
                Ok(())
            }
        }
    }

    fn evaluate_expression(
        &mut self,
        expression: &Expression,
        environment: &Environment,
    ) -> Result<Value, MeterError> {
        self.tick_fuel()?;
        match expression {
            Expression::I64(value) => Ok(Value::I64(*value)),
            Expression::Bool(value) => Ok(Value::Bool(*value)),
            Expression::Variable(name) => environment.lookup(name),
            Expression::Field(base, index) => {
                let Value::Tuple(tuple) = self.evaluate_expression(base, environment)? else {
                    return Err(MeterError::InvalidProgram("field_on_non_tuple"));
                };
                let field = tuple
                    .fields
                    .get(usize::from(*index))
                    .ok_or(MeterError::InvalidProgram("field_out_of_bounds"))?;
                self.tick_operation()?;
                Ok(match field {
                    TupleField::Signed(value) => Value::NumericField(*value),
                    TupleField::Unsigned(value) => Value::NumericField(*value as i64),
                    TupleField::Bool(value) => Value::Bool(*value),
                })
            }
            Expression::CastI64(inner) => {
                let value = self.evaluate_expression(inner, environment)?;
                self.tick_operation()?;
                match value {
                    Value::I64(value) | Value::NumericField(value) => Ok(Value::I64(value)),
                    _ => Err(MeterError::InvalidProgram("invalid_i64_cast")),
                }
            }
            Expression::Wrapping {
                operation,
                left,
                right,
            } => {
                let Value::I64(left) = self.evaluate_expression(left, environment)? else {
                    return Err(MeterError::InvalidProgram("non_i64_wrapping_operand"));
                };
                let Value::I64(right) = self.evaluate_expression(right, environment)? else {
                    return Err(MeterError::InvalidProgram("non_i64_wrapping_operand"));
                };
                self.tick_operation()?;
                Ok(Value::I64(match operation {
                    WrappingOperation::Add => left.wrapping_add(right),
                    WrappingOperation::Sub => left.wrapping_sub(right),
                    WrappingOperation::Mul => left.wrapping_mul(right),
                }))
            }
            Expression::If {
                condition,
                then_expression,
                else_expression,
            } => {
                let Value::Bool(condition) = self.evaluate_expression(condition, environment)?
                else {
                    return Err(MeterError::InvalidProgram("non_bool_condition"));
                };
                self.tick_operation()?;
                if condition {
                    self.evaluate_expression(then_expression, environment)
                } else {
                    self.evaluate_expression(else_expression, environment)
                }
            }
        }
    }

    fn tick_operation(&mut self) -> Result<(), MeterError> {
        self.use_.operations = checked_add(self.use_.operations, 1)?;
        ensure_limit(
            self.use_.operations,
            self.limits.max_operations,
            "operations",
        )
    }

    fn tick_fuel(&mut self) -> Result<(), MeterError> {
        self.use_.fuel = checked_add(self.use_.fuel, 1)?;
        ensure_limit(self.use_.fuel, self.limits.max_fuel, "fuel")
    }
}
