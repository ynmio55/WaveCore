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
        if self.match_token(&TokenKind::Let) || self.match_token(&TokenKind::Const) || self.match_token(&TokenKind::Var) {
            self.var_declaration()
        } else if self.match_token(&TokenKind::Function) {
            self.function_declaration()
        } else {
            self.statement()
        }
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

    fn function_declaration(&mut self) -> Result<Stmt, String> {
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
                    _ => return Err(format!("Expected parameter name at line {}", self.previous().line)),
                }
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RightParen, "Expected ')' after parameters")?;

        self.consume(&TokenKind::LeftBrace, "Expected '{' before function body")?;
        let body = self.block_statement()?;
        Ok(Stmt::FunctionDecl { name, params, body })
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
        } else if self.match_token(&TokenKind::LeftBrace) {
            Ok(Stmt::Block(self.block_statement()?))
        } else {
            self.expression_statement()
        }
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
        } else if self.match_token(&TokenKind::Let) || self.match_token(&TokenKind::Var) {
            Some(Box::new(self.var_declaration()?))
        } else {
            Some(Box::new(self.expression_statement()?))
        };

        let condition = if !self.check(&TokenKind::Semicolon) {
            Some(self.expression()?)
        } else {
            None
        };
        self.consume(&TokenKind::Semicolon, "Expected ';' after loop condition")?;

        let update = if !self.check(&TokenKind::RightParen) {
            Some(self.expression()?)
        } else {
            None
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
        let expr = self.or()?;

        if self.match_token(&TokenKind::Equal) {
            let value = Box::new(self.assignment()?);
            match expr {
                Expr::Identifier(target) => Ok(Expr::Assign { target, value }),
                Expr::Member { object, property } => Ok(Expr::AssignProp {
                    object,
                    property,
                    value,
                }),
                _ => Err(format!("Invalid assignment target at line {}", self.previous().line)),
            }
        } else {
            Ok(expr)
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
                    _ => return Err(format!("Expected property name after '.' at line {}", self.previous().line)),
                };
                expr = Expr::Member {
                    object: Box::new(expr),
                    property,
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
            _ => Err(format!("Unexpected token {:?} at line {}", token.kind, token.line)),
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
