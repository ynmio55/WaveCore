use crate::ast::*;
use crate::value::JsValue;

#[derive(Debug, Clone, PartialEq)]
pub enum OpCode {
    Constant(u16),
    Null,
    Undefined,
    True,
    False,
    Pop,
    GetGlobal(String),
    SetGlobal(String),
    GetLocal(u16),
    SetLocal(u16),
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Not,
    Negate,
    Jump(usize),
    JumpIfFalse(usize),
    Loop(usize),
    Call(usize),
    Return,
    GetProp(String),
    SetProp(String),
}

#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub code: Vec<OpCode>,
    pub constants: Vec<JsValue>,
}

impl Chunk {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_op(&mut self, op: OpCode) -> usize {
        self.code.push(op);
        self.code.len() - 1
    }

    pub fn add_constant(&mut self, value: JsValue) -> u16 {
        self.constants.push(value);
        (self.constants.len() - 1) as u16
    }
}

pub struct Compiler {
    pub chunks: Vec<Chunk>,
    pub current_chunk: usize,
    locals: Vec<Vec<String>>,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            chunks: vec![Chunk::new()],
            current_chunk: 0,
            locals: vec![Vec::new()],
        }
    }

    pub fn compile(mut self, statements: &[Stmt]) -> Result<Vec<Chunk>, String> {
        let len = statements.len();
        for (i, stmt) in statements.iter().enumerate() {
            if i == len - 1 {
                if let Stmt::Expr(expr) = stmt {
                    self.compile_expr(expr)?;
                    self.chunk_mut().write_op(OpCode::Return);
                    return Ok(self.chunks);
                }
            }
            self.compile_stmt(stmt)?;
        }
        self.chunk_mut().write_op(OpCode::Undefined);
        self.chunk_mut().write_op(OpCode::Return);
        Ok(self.chunks)
    }

    fn chunk_mut(&mut self) -> &mut Chunk {
        &mut self.chunks[self.current_chunk]
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::VarDecl { name, initializer } => {
                if let Some(init) = initializer {
                    self.compile_expr(init)?;
                } else {
                    self.chunk_mut().write_op(OpCode::Undefined);
                }

                if self.locals.last().unwrap().is_empty() && self.current_chunk == 0 {
                    // Global variable
                    self.chunk_mut().write_op(OpCode::SetGlobal(name.clone()));
                } else {
                    // Local variable in current frame
                    let idx = self.locals.last().unwrap().len() as u16;
                    self.locals.last_mut().unwrap().push(name.clone());
                    self.chunk_mut().write_op(OpCode::SetLocal(idx));
                }
            }
            Stmt::Expr(expr) => {
                self.compile_expr(expr)?;
                self.chunk_mut().write_op(OpCode::Pop);
            }
            Stmt::Block(stmts) => {
                for s in stmts {
                    self.compile_stmt(s)?;
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.compile_expr(condition)?;
                let then_jump = self.chunk_mut().write_op(OpCode::JumpIfFalse(0));
                self.chunk_mut().write_op(OpCode::Pop); // pop condition

                self.compile_stmt(then_branch)?;

                let else_jump = self.chunk_mut().write_op(OpCode::Jump(0));

                // Patch then_jump
                let after_then = self.chunk_mut().code.len();
                self.chunk_mut().code[then_jump] = OpCode::JumpIfFalse(after_then);

                self.chunk_mut().write_op(OpCode::Pop); // pop condition on false path

                if let Some(else_b) = else_branch {
                    self.compile_stmt(else_b)?;
                }

                let after_else = self.chunk_mut().code.len();
                self.chunk_mut().code[else_jump] = OpCode::Jump(after_else);
            }
            Stmt::While { condition, body } => {
                let loop_start = self.chunk_mut().code.len();
                self.compile_expr(condition)?;
                let exit_jump = self.chunk_mut().write_op(OpCode::JumpIfFalse(0));
                self.chunk_mut().write_op(OpCode::Pop);

                self.compile_stmt(body)?;
                self.chunk_mut().write_op(OpCode::Loop(loop_start));

                let after_loop = self.chunk_mut().code.len();
                self.chunk_mut().code[exit_jump] = OpCode::JumpIfFalse(after_loop);
                self.chunk_mut().write_op(OpCode::Pop);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.compile_stmt(i)?;
                }
                let loop_start = self.chunk_mut().code.len();
                let exit_jump = if let Some(cond) = condition {
                    self.compile_expr(cond)?;
                    let j = self.chunk_mut().write_op(OpCode::JumpIfFalse(0));
                    self.chunk_mut().write_op(OpCode::Pop);
                    Some(j)
                } else {
                    None
                };

                self.compile_stmt(body)?;

                if let Some(up) = update {
                    self.compile_expr(up)?;
                    self.chunk_mut().write_op(OpCode::Pop);
                }

                self.chunk_mut().write_op(OpCode::Loop(loop_start));

                if let Some(exit) = exit_jump {
                    let after_loop = self.chunk_mut().code.len();
                    self.chunk_mut().code[exit] = OpCode::JumpIfFalse(after_loop);
                    self.chunk_mut().write_op(OpCode::Pop);
                }
            }
            Stmt::FunctionDecl { name, params, body } => {
                let func_chunk_idx = self.chunks.len();
                self.chunks.push(Chunk::new());
                let old_chunk = self.current_chunk;
                self.current_chunk = func_chunk_idx;

                self.locals.push(params.clone());
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.chunk_mut().write_op(OpCode::Undefined);
                self.chunk_mut().write_op(OpCode::Return);
                self.locals.pop();

                self.current_chunk = old_chunk;

                let func_obj = crate::value::JsFunction {
                    name: name.clone(),
                    params: params.clone(),
                    chunk_index: func_chunk_idx,
                };
                let c_idx = self
                    .chunk_mut()
                    .add_constant(JsValue::Function(std::rc::Rc::new(func_obj)));
                self.chunk_mut().write_op(OpCode::Constant(c_idx));
                self.chunk_mut().write_op(OpCode::SetGlobal(name.clone()));
            }
            Stmt::Return(val) => {
                if let Some(expr) = val {
                    self.compile_expr(expr)?;
                } else {
                    self.chunk_mut().write_op(OpCode::Undefined);
                }
                self.chunk_mut().write_op(OpCode::Return);
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Number(n) => {
                let idx = self.chunk_mut().add_constant(JsValue::Number(*n));
                self.chunk_mut().write_op(OpCode::Constant(idx));
            }
            Expr::String(s) => {
                let idx = self.chunk_mut().add_constant(JsValue::String(s.clone()));
                self.chunk_mut().write_op(OpCode::Constant(idx));
            }
            Expr::Boolean(b) => {
                if *b {
                    self.chunk_mut().write_op(OpCode::True);
                } else {
                    self.chunk_mut().write_op(OpCode::False);
                }
            }
            Expr::Null => {
                self.chunk_mut().write_op(OpCode::Null);
            }
            Expr::Undefined => {
                self.chunk_mut().write_op(OpCode::Undefined);
            }
            Expr::Identifier(name) => {
                if let Some(local_idx) = self.resolve_local(name) {
                    self.chunk_mut().write_op(OpCode::GetLocal(local_idx));
                } else {
                    self.chunk_mut().write_op(OpCode::GetGlobal(name.clone()));
                }
            }
            Expr::Assign { target, value } => {
                self.compile_expr(value)?;
                if let Some(local_idx) = self.resolve_local(target) {
                    self.chunk_mut().write_op(OpCode::SetLocal(local_idx));
                } else {
                    self.chunk_mut().write_op(OpCode::SetGlobal(target.clone()));
                }
            }
            Expr::AssignProp {
                object,
                property,
                value,
            } => {
                self.compile_expr(object)?;
                self.compile_expr(value)?;
                self.chunk_mut().write_op(OpCode::SetProp(property.clone()));
            }
            Expr::Member { object, property } => {
                self.compile_expr(object)?;
                self.chunk_mut().write_op(OpCode::GetProp(property.clone()));
            }
            Expr::Binary { op, left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                match op {
                    BinaryOp::Add => self.chunk_mut().write_op(OpCode::Add),
                    BinaryOp::Sub => self.chunk_mut().write_op(OpCode::Sub),
                    BinaryOp::Mul => self.chunk_mut().write_op(OpCode::Mul),
                    BinaryOp::Div => self.chunk_mut().write_op(OpCode::Div),
                    BinaryOp::Mod => self.chunk_mut().write_op(OpCode::Mod),
                    BinaryOp::Equal => self.chunk_mut().write_op(OpCode::Equal),
                    BinaryOp::NotEqual => self.chunk_mut().write_op(OpCode::NotEqual),
                    BinaryOp::StrictEqual => self.chunk_mut().write_op(OpCode::StrictEqual),
                    BinaryOp::StrictNotEqual => self.chunk_mut().write_op(OpCode::StrictNotEqual),
                    BinaryOp::Greater => self.chunk_mut().write_op(OpCode::Greater),
                    BinaryOp::GreaterEqual => self.chunk_mut().write_op(OpCode::GreaterEqual),
                    BinaryOp::Less => self.chunk_mut().write_op(OpCode::Less),
                    BinaryOp::LessEqual => self.chunk_mut().write_op(OpCode::LessEqual),
                    BinaryOp::And => self.chunk_mut().write_op(OpCode::Equal), // Fallback
                    BinaryOp::Or => self.chunk_mut().write_op(OpCode::Equal),
                };
            }
            Expr::Unary { op, expr } => {
                self.compile_expr(expr)?;
                match op {
                    UnaryOp::Not => self.chunk_mut().write_op(OpCode::Not),
                    UnaryOp::Negate => self.chunk_mut().write_op(OpCode::Negate),
                };
            }
            Expr::Call { callee, args } => {
                self.compile_expr(callee)?;
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk_mut().write_op(OpCode::Call(args.len()));
            }
        }
        Ok(())
    }

    fn resolve_local(&self, name: &str) -> Option<u16> {
        let frame = self.locals.last()?;
        for (i, n) in frame.iter().enumerate().rev() {
            if n == name {
                return Some(i as u16);
            }
        }
        None
    }
}
