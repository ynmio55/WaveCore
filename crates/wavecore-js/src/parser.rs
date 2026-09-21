use crate::ast::*;
use crate::lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0 }
    }

    pub fn parse(&mut self) -> Result<Vec<Stmt>, String> {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            statements.push(self.declaration()?);
        }
        Ok(statements)
    }

    fn declaration(&mut self) -> Result<Stmt, String> {
        if self.match_token(&TokenKind::Let)
            || self.match_token(&TokenKind::Const)
            || self.match_token(&TokenKind::Var)
        {
            self.var_declaration()
        } else if self.match_token(&TokenKind::Async) {
            self.consume(&TokenKind::Function, "Expected 'function' after 'async'")?;
            self.function_declaration(true)
        } else if self.match_token(&TokenKind::Function) {
            self.function_declaration(false)
        } else if self.match_token(&TokenKind::Class) {
            self.class_declaration()
        } else {
            self.statement()
        }
    }

    fn class_declaration(&mut self) -> Result<Stmt, String> {
        let name = match self.advance().kind {
            TokenKind::Identifier(name) => name,
            _ => return Err(format!("Expected class name at line {}", self.previous().line)),
        };
        self.consume(&TokenKind::LeftBrace, "Expected '{' after class name")?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            let method_name = match self.advance().kind {
                TokenKind::Identifier(name) => name,
                _ => {
                    return Err(format!(
                        "Expected method name in class '{}' at line {}",
                        name,
                        self.previous().line
                    ))
                }
            };
            self.consume(&TokenKind::LeftParen, "Expected '(' after method name")?;
            let mut params = Vec::new();
            if !self.check(&TokenKind::RightParen) {
                loop {
                    match self.advance().kind {
                        TokenKind::Identifier(param) => params.push(param),
                        _ => {
                            return Err(format!(
                                "Expected parameter name at line {}",
                                self.previous().line
                            ))
                        }
                    }
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RightParen, "Expected ')' after method parameters")?;
            self.consume(&TokenKind::LeftBrace, "Expected '{' before method body")?;
            let body = self.block_statement()?;
            methods.push(ClassMethod {
                name: method_name,
                params,
                body,
            });
        }

        self.consume(&TokenKind::RightBrace, "Expected '}' after class body")?;
        Ok(Stmt::ClassDecl { name, methods })
    }

    fn var_declaration(&mut self) -> Result<Stmt, String> {
        let name = match self.advance().kind {
            TokenKind::Identifier(n) => n,
            _ => return Err(format!("Expected variable name at line {}", self.previous().line)),
        };

        let initializer = if self.match_token(&TokenKind::Equal) {
            Some(self.expression()?)
        } else {
            None
        };

        self.match_token(&TokenKind::Semicolon);
        Ok(Stmt::VarDecl { name, initializer })
    }

    fn function_declaration(&mut self, is_async: bool) -> Result<Stmt, String> {
        let name = match self.advance().kind {
            TokenKind::Identifier(n) => n,
            _ => return Err(format!("Expected function name at line {}", self.previous().line)),
        };

        self.consume(&TokenKind::LeftParen, "Expected '(' after function name")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::RightParen) {
            loop {
                match self.advance().kind {
                    TokenKind::Identifier(p) => params.push(p),
                    _ => {
                        return Err(format!(
                            "Expected parameter name at line {}",
                            self.previous().line
                        ))
                    }
                }
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RightParen, "Expected ')' after parameters")?;

        self.consume(&TokenKind::LeftBrace, "Expected '{' before function body")?;
        let body = self.block_statement()?;
        Ok(Stmt::FunctionDecl {
            name,
            params,
            body,
            is_async,
        })
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        if self.match_token(&TokenKind::If) {
            self.if_statement()
        } else if self.match_token(&TokenKind::While) {
            self.while_statement()
        } else if self.match_token(&TokenKind::For) {
            self.for_statement()
        } else if self.match_token(&TokenKind::Return) {
            self.return_statement()
        } else if self.match_token(&TokenKind::Try) {
            self.try_statement()
        } else if self.match_token(&TokenKind::Throw) {
            self.throw_statement()
        } else if self.match_token(&TokenKind::LeftBrace) {
            Ok(Stmt::Block(self.block_statement()?))
        } else {
            self.expression_statement()
        }
    }

    fn try_statement(&mut self) -> Result<Stmt, String> {
        self.consume(&TokenKind::LeftBrace, "Expected '{' after 'try'")?;
        let try_block = Box::new(Stmt::Block(self.block_statement()?));

        let mut catch_param = None;
        let mut catch_block = None;
        if self.match_token(&TokenKind::Catch) {
            if self.match_token(&TokenKind::LeftParen) {
                let param = match self.advance().kind {
                    TokenKind::Identifier(p) => p,
                    _ => {
                        return Err(format!(
                            "Expected catch parameter name at line {}",
                            self.previous().line
                        ))
                    }
                };
                self.consume(&TokenKind::RightParen, "Expected ')' after catch parameter")?;
                catch_param = Some(param);
            }
            self.consume(&TokenKind::LeftBrace, "Expected '{' before catch block")?;
            catch_block = Some(Box::new(Stmt::Block(self.block_statement()?)));
        }

        let mut finally_block = None;
        if self.match_token(&TokenKind::Finally) {
            self.consume(&TokenKind::LeftBrace, "Expected '{' before finally block")?;
            finally_block = Some(Box::new(Stmt::Block(self.block_statement()?)));
        }

        if catch_block.is_none() && finally_block.is_none() {
            return Err("Missing catch or finally clause after try".to_string());
        }

        Ok(Stmt::TryCatch {
            try_block,
            catch_param,
            catch_block,
            finally_block,
        })
    }

    fn throw_statement(&mut self) -> Result<Stmt, String> {
        let expr = self.expression()?;
        self.match_token(&TokenKind::Semicolon);
        Ok(Stmt::Throw(expr))
    }

    fn if_statement(&mut self) -> Result<Stmt, String> {
        self.consume(&TokenKind::LeftParen, "Expected '(' after 'if'")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::RightParen, "Expected ')' after condition")?;

        let then_branch = Box::new(self.statement()?);
        let else_branch = if self.match_token(&TokenKind::Else) {
            Some(Box::new(self.statement()?))
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn while_statement(&mut self) -> Result<Stmt, String> {
        self.consume(&TokenKind::LeftParen, "Expected '(' after 'while'")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::RightParen, "Expected ')' after condition")?;
        let body = Box::new(self.statement()?);
        Ok(Stmt::While { condition, body })
    }

    fn for_statement(&mut self) -> Result<Stmt, String> {
        self.consume(&TokenKind::LeftParen, "Expected '(' after 'for'")?;

        let init = if self.match_token(&TokenKind::Semicolon) {
            None
        } else if self.match_token(&TokenKind::Let)
            || self.match_token(&TokenKind::Var)
            || self.match_token(&TokenKind::Const)
        {
            Some(Box::new(self.var_declaration()?))
        } else {
            let expr = self.expression()?;
            self.consume(&TokenKind::Semicolon, "Expected ';' after for init")?;
            Some(Box::new(Stmt::Expr(expr)))
        };

        let condition = if self.check(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.expression()?)
        };
        self.consume(&TokenKind::Semicolon, "Expected ';' after for condition")?;

        let update = if self.check(&TokenKind::RightParen) {
            None
        } else {
            Some(self.expression()?)
        };
        self.consume(&TokenKind::RightParen, "Expected ')' after for clauses")?;

        let body = Box::new(self.statement()?);

        Ok(Stmt::For {
            init,
            condition,
            update,
            body,
        })
    }

    fn return_statement(&mut self) -> Result<Stmt, String> {
        let value = if !self.check(&TokenKind::Semicolon) && !self.check(&TokenKind::RightBrace) {
            Some(self.expression()?)
        } else {
            None
        };
        self.match_token(&TokenKind::Semicolon);
        Ok(Stmt::Return(value))
    }

    fn block_statement(&mut self) -> Result<Vec<Stmt>, String> {
        let mut statements = Vec::new();
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            statements.push(self.declaration()?);
        }
        self.consume(&TokenKind::RightBrace, "Expected '}' after block")?;
        Ok(statements)
    }

    fn expression_statement(&mut self) -> Result<Stmt, String> {
        let expr = self.expression()?;
        self.match_token(&TokenKind::Semicolon);
        Ok(Stmt::Expr(expr))
    }

    pub fn expression(&mut self) -> Result<Expr, String> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr, String> {
        let expr = self.conditional()?;

        if self.match_token(&TokenKind::Equal) {
            let value = Box::new(self.assignment()?);
            match expr {
                Expr::Identifier(target) => Ok(Expr::Assign { target, value }),
                Expr::Member { object, property } => {
                    Ok(Expr::AssignProp { object, property, value })
                }
                Expr::Index { object, index } => {
                    Ok(Expr::AssignIndex { object, index, value })
                }
                _ => Err(format!(
                    "Invalid assignment target at line {}",
                    self.previous().line
                )),
            }
        } else if self.match_token(&TokenKind::PlusEqual) {
            let val = Box::new(self.assignment()?);
            self.make_compound_assign(expr, BinaryOp::Add, val)
        } else if self.match_token(&TokenKind::MinusEqual) {
            let val = Box::new(self.assignment()?);
            self.make_compound_assign(expr, BinaryOp::Sub, val)
        } else if self.match_token(&TokenKind::StarEqual) {
            let val = Box::new(self.assignment()?);
            self.make_compound_assign(expr, BinaryOp::Mul, val)
        } else if self.match_token(&TokenKind::SlashEqual) {
            let val = Box::new(self.assignment()?);
            self.make_compound_assign(expr, BinaryOp::Div, val)
        } else {
            Ok(expr)
        }
    }

    fn make_compound_assign(
        &self,
        target: Expr,
        op: BinaryOp,
        val: Box<Expr>,
    ) -> Result<Expr, String> {
        match target {
            Expr::Identifier(name) => Ok(Expr::Assign {
                target: name.clone(),
                value: Box::new(Expr::Binary {
                    op,
                    left: Box::new(Expr::Identifier(name)),
                    right: val,
                }),
            }),
            Expr::Member { object, property } => Ok(Expr::AssignProp {
                object: object.clone(),
                property: property.clone(),
                value: Box::new(Expr::Binary {
                    op,
                    left: Box::new(Expr::Member { object, property }),
                    right: val,
                }),
            }),
            Expr::Index { object, index } => Ok(Expr::AssignIndex {
                object: object.clone(),
                index: index.clone(),
                value: Box::new(Expr::Binary {
                    op,
                    left: Box::new(Expr::Index { object, index }),
                    right: val,
                }),
            }),
            _ => Err("Invalid compound assignment target".to_string()),
        }
    }

    fn conditional(&mut self) -> Result<Expr, String> {
        let condition = self.or()?;
        if self.match_token(&TokenKind::Question) {
            let then_expr = self.assignment()?;
            self.consume(&TokenKind::Colon, "Expected ':' in conditional expression")?;
            let else_expr = self.assignment()?;
            Ok(Expr::Conditional {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            })
        } else {
            Ok(condition)
        }
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut expr = self.and()?;
        while self.match_token(&TokenKind::OrOr) {
            let right = Box::new(self.and()?);
            expr = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(expr),
                right,
            };
        }
        Ok(expr)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut expr = self.equality()?;
        while self.match_token(&TokenKind::AndAnd) {
            let right = Box::new(self.equality()?);
            expr = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(expr),
                right,
            };
        }
        Ok(expr)
    }

    fn equality(&mut self) -> Result<Expr, String> {
        let mut expr = self.comparison()?;
        while let Some(op) = self.match_equality_op() {
            let right = Box::new(self.comparison()?);
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right,
            };
        }
        Ok(expr)
    }

    fn match_equality_op(&mut self) -> Option<BinaryOp> {
        if self.match_token(&TokenKind::EqualEqual) {
            Some(BinaryOp::Equal)
        } else if self.match_token(&TokenKind::EqualEqualEqual) {
            Some(BinaryOp::StrictEqual)
        } else if self.match_token(&TokenKind::NotEqual) {
            Some(BinaryOp::NotEqual)
        } else if self.match_token(&TokenKind::NotEqualEqual) {
            Some(BinaryOp::StrictNotEqual)
        } else {
            None
        }
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let mut expr = self.term()?;
        while let Some(op) = self.match_comparison_op() {
            let right = Box::new(self.term()?);
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right,
            };
        }
        Ok(expr)
    }

    fn match_comparison_op(&mut self) -> Option<BinaryOp> {
        if self.match_token(&TokenKind::Greater) {
            Some(BinaryOp::Greater)
        } else if self.match_token(&TokenKind::GreaterEqual) {
            Some(BinaryOp::GreaterEqual)
        } else if self.match_token(&TokenKind::Less) {
            Some(BinaryOp::Less)
        } else if self.match_token(&TokenKind::LessEqual) {
            Some(BinaryOp::LessEqual)
        } else if self.match_token(&TokenKind::InstanceOf) {
            Some(BinaryOp::InstanceOf)
        } else if self.match_token(&TokenKind::In) {
            Some(BinaryOp::In)
        } else {
            None
        }
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut expr = self.factor()?;
        while let Some(op) = self.match_term_op() {
            let right = Box::new(self.factor()?);
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right,
            };
        }
        Ok(expr)
    }

    fn match_term_op(&mut self) -> Option<BinaryOp> {
        if self.match_token(&TokenKind::Plus) {
            Some(BinaryOp::Add)
        } else if self.match_token(&TokenKind::Minus) {
            Some(BinaryOp::Sub)
        } else {
            None
        }
    }

    fn factor(&mut self) -> Result<Expr, String> {
        let mut expr = self.unary()?;
        while let Some(op) = self.match_factor_op() {
            let right = Box::new(self.unary()?);
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right,
            };
        }
        Ok(expr)
    }

    fn match_factor_op(&mut self) -> Option<BinaryOp> {
        if self.match_token(&TokenKind::Star) {
            Some(BinaryOp::Mul)
        } else if self.match_token(&TokenKind::Slash) {
            Some(BinaryOp::Div)
        } else if self.match_token(&TokenKind::Percent) {
            Some(BinaryOp::Mod)
        } else {
            None
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.match_token(&TokenKind::Bang) {
            let expr = Box::new(self.unary()?);
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr,
            })
        } else if self.match_token(&TokenKind::Minus) {
            let expr = Box::new(self.unary()?);
            Ok(Expr::Unary {
                op: UnaryOp::Negate,
                expr,
            })
        } else if self.match_token(&TokenKind::TypeOf) {
            let expr = Box::new(self.unary()?);
            Ok(Expr::Unary {
                op: UnaryOp::TypeOf,
                expr,
            })
        } else if self.match_token(&TokenKind::Await) {
            Ok(Expr::Await(Box::new(self.unary()?)))
        } else if self.match_token(&TokenKind::New) {
            let callee = self.call()?;
            match callee {
                Expr::Call { callee, args } => Ok(Expr::New { callee, args }),
                other => Ok(Expr::New {
                    callee: Box::new(other),
                    args: Vec::new(),
                }),
            }
        } else {
            self.call()
        }
    }

    fn call(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;

        loop {
            if self.match_token(&TokenKind::LeftParen) {
                let mut args = Vec::new();
                if !self.check(&TokenKind::RightParen) {
                    loop {
                        args.push(self.expression()?);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RightParen, "Expected ')' after arguments")?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.match_token(&TokenKind::Dot) {
                let property = match self.advance().kind {
                    TokenKind::Identifier(p) => p,
                    _ => {
                        return Err(format!(
                            "Expected property name after '.' at line {}",
                            self.previous().line
                        ))
                    }
                };
                expr = Expr::Member {
                    object: Box::new(expr),
                    property,
                };
            } else if self.match_token(&TokenKind::LeftBracket) {
                let index = self.expression()?;
                self.consume(&TokenKind::RightBracket, "Expected ']' after index")?;
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                };
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        if self.match_token(&TokenKind::False) {
            return Ok(Expr::Boolean(false));
        }
        if self.match_token(&TokenKind::True) {
            return Ok(Expr::Boolean(true));
        }
        if self.match_token(&TokenKind::Null) {
            return Ok(Expr::Null);
        }
        if self.match_token(&TokenKind::Undefined) {
            return Ok(Expr::Undefined);
        }
        if self.match_token(&TokenKind::This) {
            return Ok(Expr::This);
        }

        // Array literal [a, b, c]
        if self.match_token(&TokenKind::LeftBracket) {
            let mut items = Vec::new();
            if !self.check(&TokenKind::RightBracket) {
                loop {
                    items.push(self.expression()?);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RightBracket, "Expected ']' after array elements")?;
            return Ok(Expr::Array(items));
        }

        // Object literal { key: val, ... }
        if self.match_token(&TokenKind::LeftBrace) {
            let mut entries = Vec::new();
            if !self.check(&TokenKind::RightBrace) {
                loop {
                    let key = match self.advance().kind {
                        TokenKind::Identifier(k) => k,
                        TokenKind::String(s) => s,
                        _ => {
                            return Err(format!(
                                "Expected property key in object literal at line {}",
                                self.previous().line
                            ))
                        }
                    };
                    self.consume(&TokenKind::Colon, "Expected ':' after property key")?;
                    let val = self.expression()?;
                    entries.push((key, val));
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RightBrace, "Expected '}' after object properties")?;
            return Ok(Expr::Object(entries));
        }

        // Function expression: [async] function [name](params) { body }
        let mut is_async_function = false;
        if self.match_token(&TokenKind::Async) {
            self.consume(&TokenKind::Function, "Expected 'function' after 'async'")?;
            is_async_function = true;
        }
        if is_async_function || self.match_token(&TokenKind::Function) {
            let name = if let TokenKind::Identifier(n) = &self.peek().kind {
                let n = n.clone();
                self.advance();
                Some(n)
            } else {
                None
            };
            self.consume(&TokenKind::LeftParen, "Expected '(' after function")?;
            let mut params = Vec::new();
            if !self.check(&TokenKind::RightParen) {
                loop {
                    match self.advance().kind {
                        TokenKind::Identifier(p) => params.push(p),
                        _ => {
                            return Err(format!(
                                "Expected parameter name at line {}",
                                self.previous().line
                            ))
                        }
                    }
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RightParen, "Expected ')' after parameters")?;
            self.consume(&TokenKind::LeftBrace, "Expected '{' before function body")?;
            let body = self.block_statement()?;
            return Ok(Expr::FunctionExpr {
                name,
                params,
                body,
                is_async: is_async_function,
            });
        }

        let token = self.advance();
        match token.kind {
            TokenKind::Number(n) => Ok(Expr::Number(n)),
            TokenKind::String(s) => Ok(Expr::String(s)),
            TokenKind::Identifier(name) => Ok(Expr::Identifier(name)),
            TokenKind::LeftParen => {
                let expr = self.expression()?;
                self.consume(&TokenKind::RightParen, "Expected ')' after expression")?;
                Ok(expr)
            }
            _ => Err(format!(
                "Unexpected token {:?} at line {}",
                token.kind, token.line
            )),
        }
    }

    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() {
            false
        } else {
            &self.peek().kind == kind
        }
    }

    fn advance(&mut self) -> Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn previous(&self) -> Token {
        self.tokens[self.current - 1].clone()
    }

    fn consume(&mut self, kind: &TokenKind, message: &str) -> Result<Token, String> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(format!("{} at line {}", message, self.peek().line))
        }
    }
}
