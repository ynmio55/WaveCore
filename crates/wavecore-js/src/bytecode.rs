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
    GetVar(String),
    SetVar(String),
    DeclVar(String),
    GetGlobal(String),
    SetGlobal(String),
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
    TypeOf,
    Jump(usize),
    JumpIfFalse(usize),
    Loop(usize),
    Call(usize),
    Construct(usize),
    Return,
    GetProp(String),
    SetProp(String),
    GetIndex,
    SetIndex,
    CreateArray(usize),
    CreateObject(usize),
    MakeClosure {
        chunk_index: usize,
        name: String,
        params: Vec<String>,
    },
    MakeClass {
        name: String,
        constructor: Option<(usize, Vec<String>)>,
        methods: Vec<(String, usize, Vec<String>)>,
    },
    PushTry {
        catch_ip: usize,
    },
    PopTry,
    Throw,
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
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            chunks: vec![Chunk::new()],
            current_chunk: 0,
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
                self.chunk_mut().write_op(OpCode::DeclVar(name.clone()));
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
                self.chunk_mut().write_op(OpCode::Pop);

                self.compile_stmt(then_branch)?;

                let else_jump = self.chunk_mut().write_op(OpCode::Jump(0));
                let after_then = self.chunk_mut().code.len();
                self.chunk_mut().code[then_jump] = OpCode::JumpIfFalse(after_then);
                self.chunk_mut().write_op(OpCode::Pop);

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

                for s in body {
                    self.compile_stmt(s)?;
                }
                self.chunk_mut().write_op(OpCode::Undefined);
                self.chunk_mut().write_op(OpCode::Return);

                self.current_chunk = old_chunk;

                self.chunk_mut().write_op(OpCode::MakeClosure {
                    chunk_index: func_chunk_idx,
                    name: name.clone(),
                    params: params.clone(),
                });
                self.chunk_mut().write_op(OpCode::DeclVar(name.clone()));
            }
            Stmt::ClassDecl { name, methods } => {
                let old_chunk = self.current_chunk;
                let mut constructor = None;
                let mut compiled_methods = Vec::new();

                for method in methods {
                    let method_chunk_idx = self.chunks.len();
                    self.chunks.push(Chunk::new());
                    self.current_chunk = method_chunk_idx;
                    for statement in &method.body {
                        self.compile_stmt(statement)?;
                    }
                    self.chunk_mut().write_op(OpCode::Undefined);
                    self.chunk_mut().write_op(OpCode::Return);
                    self.current_chunk = old_chunk;

                    if method.name == "constructor" {
                        constructor = Some((method_chunk_idx, method.params.clone()));
                    } else {
                        compiled_methods.push((
                            method.name.clone(),
                            method_chunk_idx,
                            method.params.clone(),
                        ));
                    }
                }

                self.current_chunk = old_chunk;
                self.chunk_mut().write_op(OpCode::MakeClass {
                    name: name.clone(),
                    constructor,
                    methods: compiled_methods,
                });
                self.chunk_mut().write_op(OpCode::DeclVar(name.clone()));
            }
            Stmt::Return(val) => {
                if let Some(expr) = val {
                    self.compile_expr(expr)?;
                } else {
                    self.chunk_mut().write_op(OpCode::Undefined);
                }
                self.chunk_mut().write_op(OpCode::Return);
            }
            Stmt::TryCatch {
                try_block,
                catch_param,
                catch_block,
                finally_block,
            } => {
                let push_try_ip = self.chunk_mut().write_op(OpCode::PushTry { catch_ip: 0 });
                self.compile_stmt(try_block)?;
                self.chunk_mut().write_op(OpCode::PopTry);

                let jump_over_catch = self.chunk_mut().write_op(OpCode::Jump(0));
                let catch_target = self.chunk_mut().code.len();
                self.chunk_mut().code[push_try_ip] = OpCode::PushTry {
                    catch_ip: catch_target,
                };

                if let Some(c_block) = catch_block {
                    if let Some(param) = catch_param {
                        self.chunk_mut().write_op(OpCode::DeclVar(param.clone()));
                    } else {
                        self.chunk_mut().write_op(OpCode::Pop);
                    }
                    self.compile_stmt(c_block)?;
                } else {
                    self.chunk_mut().write_op(OpCode::Pop);
                }

                let after_catch = self.chunk_mut().code.len();
                self.chunk_mut().code[jump_over_catch] = OpCode::Jump(after_catch);

                if let Some(f_block) = finally_block {
                    self.compile_stmt(f_block)?;
                }
            }
            Stmt::Throw(expr) => {
                self.compile_expr(expr)?;
                self.chunk_mut().write_op(OpCode::Throw);
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
            Expr::This => {
                self.chunk_mut().write_op(OpCode::GetVar("this".to_string()));
            }
            Expr::Identifier(name) => {
                self.chunk_mut().write_op(OpCode::GetVar(name.clone()));
            }
            Expr::Assign { target, value } => {
                self.compile_expr(value)?;
                self.chunk_mut().write_op(OpCode::SetVar(target.clone()));
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
            Expr::AssignIndex {
                object,
                index,
                value,
            } => {
                self.compile_expr(object)?;
                self.compile_expr(index)?;
                self.compile_expr(value)?;
                self.chunk_mut().write_op(OpCode::SetIndex);
            }
            Expr::Member { object, property } => {
                self.compile_expr(object)?;
                self.chunk_mut().write_op(OpCode::GetProp(property.clone()));
            }
            Expr::Index { object, index } => {
                self.compile_expr(object)?;
                self.compile_expr(index)?;
                self.chunk_mut().write_op(OpCode::GetIndex);
            }
            Expr::Array(items) => {
                for item in items {
                    self.compile_expr(item)?;
                }
                self.chunk_mut().write_op(OpCode::CreateArray(items.len()));
            }
            Expr::Object(entries) => {
                for (k, v) in entries {
                    let k_idx = self.chunk_mut().add_constant(JsValue::String(k.clone()));
                    self.chunk_mut().write_op(OpCode::Constant(k_idx));
                    self.compile_expr(v)?;
                }
                self.chunk_mut().write_op(OpCode::CreateObject(entries.len()));
            }
            Expr::FunctionExpr { name, params, body } => {
                let func_chunk_idx = self.chunks.len();
                self.chunks.push(Chunk::new());
                let old_chunk = self.current_chunk;
                self.current_chunk = func_chunk_idx;

                for s in body {
                    self.compile_stmt(s)?;
                }
                self.chunk_mut().write_op(OpCode::Undefined);
                self.chunk_mut().write_op(OpCode::Return);

                self.current_chunk = old_chunk;

                self.chunk_mut().write_op(OpCode::MakeClosure {
                    chunk_index: func_chunk_idx,
                    name: name.clone().unwrap_or_default(),
                    params: params.clone(),
                });
            }
            Expr::New { callee, args } => {
                self.compile_expr(callee)?;
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk_mut().write_op(OpCode::Construct(args.len()));
            }
            Expr::Binary { op, left, right } => {
                match op {
                    BinaryOp::And => {
                        self.compile_expr(left)?;
                        let end_jump = self.chunk_mut().write_op(OpCode::JumpIfFalse(0));
                        self.chunk_mut().write_op(OpCode::Pop);
                        self.compile_expr(right)?;
                        let end = self.chunk_mut().code.len();
                        self.chunk_mut().code[end_jump] = OpCode::JumpIfFalse(end);
                    }
                    BinaryOp::Or => {
                        self.compile_expr(left)?;
                        let eval_right = self.chunk_mut().write_op(OpCode::JumpIfFalse(0));
                        let end_jump = self.chunk_mut().write_op(OpCode::Jump(0));
                        let right_start = self.chunk_mut().code.len();
                        self.chunk_mut().code[eval_right] = OpCode::JumpIfFalse(right_start);
                        self.chunk_mut().write_op(OpCode::Pop);
                        self.compile_expr(right)?;
                        let end = self.chunk_mut().code.len();
                        self.chunk_mut().code[end_jump] = OpCode::Jump(end);
                    }
                    _ => {
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
                            BinaryOp::InstanceOf | BinaryOp::In => {
                                self.chunk_mut().write_op(OpCode::Equal)
                            }
                            BinaryOp::And | BinaryOp::Or => unreachable!(),
                        };
                    }
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.compile_expr(condition)?;
                let else_jump = self.chunk_mut().write_op(OpCode::JumpIfFalse(0));
                self.chunk_mut().write_op(OpCode::Pop);
                self.compile_expr(then_expr)?;
                let end_jump = self.chunk_mut().write_op(OpCode::Jump(0));

                let else_start = self.chunk_mut().code.len();
                self.chunk_mut().code[else_jump] = OpCode::JumpIfFalse(else_start);
                self.chunk_mut().write_op(OpCode::Pop);
                self.compile_expr(else_expr)?;

                let end = self.chunk_mut().code.len();
                self.chunk_mut().code[end_jump] = OpCode::Jump(end);
            }
            Expr::Unary { op, expr } => {
                self.compile_expr(expr)?;
                match op {
                    UnaryOp::Not => self.chunk_mut().write_op(OpCode::Not),
                    UnaryOp::Negate => self.chunk_mut().write_op(OpCode::Negate),
                    UnaryOp::TypeOf => self.chunk_mut().write_op(OpCode::TypeOf),
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
}
