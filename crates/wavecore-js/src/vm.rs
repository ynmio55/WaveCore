use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::{Chunk, OpCode};
use crate::value::{JsObject, JsValue};

pub struct CallFrame {
    pub chunk_index: usize,
    pub ip: usize,
    pub stack_start: usize,
}

pub struct VM {
    pub chunks: Vec<Chunk>,
    pub stack: Vec<JsValue>,
    pub frames: Vec<CallFrame>,
    pub globals: HashMap<String, JsValue>,
    pub instruction_limit: usize,
    pub instruction_count: usize,
    pub console_output: Vec<String>,
}

impl VM {
    pub fn new() -> Self {
        let mut vm = Self {
            chunks: Vec::new(),
            stack: Vec::new(),
            frames: Vec::new(),
            globals: HashMap::new(),
            instruction_limit: 1_000_000, // Security limit against infinite loops
            instruction_count: 0,
            console_output: Vec::new(),
        };

        vm.init_builtins();
        vm
    }

    fn init_builtins(&mut self) {
        // console.log
        let mut console = JsObject::new();
        console.set(
            "log",
            JsValue::native("log", |vm, args| {
                let text = args
                    .iter()
                    .map(|a| a.to_js_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("[JS console.log] {}", text);
                vm.console_output.push(text);
                Ok(JsValue::Undefined)
            }),
        );
        console.set(
            "error",
            JsValue::native("error", |vm, args| {
                let text = args
                    .iter()
                    .map(|a| a.to_js_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                eprintln!("[JS console.error] {}", text);
                vm.console_output.push(format!("ERROR: {}", text));
                Ok(JsValue::Undefined)
            }),
        );
        self.globals.insert(
            "console".to_string(),
            JsValue::Object(Rc::new(RefCell::new(console))),
        );

        // Math.floor, Math.random
        let mut math = JsObject::new();
        math.set(
            "floor",
            JsValue::native("floor", |_vm, args| {
                let val = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                Ok(JsValue::Number(val.floor()))
            }),
        );
        math.set(
            "ceil",
            JsValue::native("ceil", |_vm, args| {
                let val = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                Ok(JsValue::Number(val.ceil()))
            }),
        );
        self.globals.insert(
            "Math".to_string(),
            JsValue::Object(Rc::new(RefCell::new(math))),
        );

        // parseInt, parseFloat
        self.globals.insert(
            "parseInt".to_string(),
            JsValue::native("parseInt", |_vm, args| {
                let s = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let n = s.trim().parse::<i64>().map(|v| v as f64).unwrap_or(f64::NAN);
                Ok(JsValue::Number(n))
            }),
        );
        self.globals.insert(
            "parseFloat".to_string(),
            JsValue::native("parseFloat", |_vm, args| {
                let s = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let n = s.trim().parse::<f64>().unwrap_or(f64::NAN);
                Ok(JsValue::Number(n))
            }),
        );
    }

    pub fn set_global(&mut self, name: &str, value: JsValue) {
        self.globals.insert(name.to_string(), value);
    }

    pub fn get_global(&self, name: &str) -> Option<&JsValue> {
        self.globals.get(name)
    }

    pub fn execute(&mut self, chunks: Vec<Chunk>) -> Result<JsValue, String> {
        self.chunks = chunks;
        self.stack.clear();
        self.frames.clear();
        self.instruction_count = 0;

        self.frames.push(CallFrame {
            chunk_index: 0,
            ip: 0,
            stack_start: 0,
        });

        self.run()
    }

    fn run(&mut self) -> Result<JsValue, String> {
        while let Some(frame) = self.frames.last_mut() {
            self.instruction_count += 1;
            if self.instruction_count > self.instruction_limit {
                return Err("Execution terminated: Script exceeded maximum instruction limit (DoS protection)".into());
            }

            let chunk = &self.chunks[frame.chunk_index];
            if frame.ip >= chunk.code.len() {
                break;
            }

            let op = chunk.code[frame.ip].clone();
            frame.ip += 1;

            match op {
                OpCode::Constant(idx) => {
                    let val = chunk.constants[idx as usize].clone();
                    self.stack.push(val);
                }
                OpCode::Null => self.stack.push(JsValue::Null),
                OpCode::Undefined => self.stack.push(JsValue::Undefined),
                OpCode::True => self.stack.push(JsValue::Boolean(true)),
                OpCode::False => self.stack.push(JsValue::Boolean(false)),
                OpCode::Pop => {
                    self.stack.pop();
                }
                OpCode::GetGlobal(name) => {
                    let val = self.globals.get(&name).cloned().unwrap_or(JsValue::Undefined);
                    self.stack.push(val);
                }
                OpCode::SetGlobal(name) => {
                    let val = self.stack.last().cloned().unwrap_or(JsValue::Undefined);
                    self.globals.insert(name, val);
                }
                OpCode::GetLocal(offset) => {
                    let start = frame.stack_start;
                    let val = self
                        .stack
                        .get(start + offset as usize)
                        .cloned()
                        .unwrap_or(JsValue::Undefined);
                    self.stack.push(val);
                }
                OpCode::SetLocal(offset) => {
                    let start = frame.stack_start;
                    let val = self.stack.last().cloned().unwrap_or(JsValue::Undefined);
                    let target = start + offset as usize;
                    if target < self.stack.len() {
                        self.stack[target] = val;
                    }
                }
                OpCode::Add => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined);
                    match (&a, &b) {
                        (JsValue::String(s1), _) => {
                            self.stack.push(JsValue::String(format!("{}{}", s1, b.to_js_string())));
                        }
                        (_, JsValue::String(s2)) => {
                            self.stack.push(JsValue::String(format!("{}{}", a.to_js_string(), s2)));
                        }
                        _ => {
                            self.stack.push(JsValue::Number(a.to_number() + b.to_number()));
                        }
                    }
                }
                OpCode::Sub => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    self.stack.push(JsValue::Number(a - b));
                }
                OpCode::Mul => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    self.stack.push(JsValue::Number(a * b));
                }
                OpCode::Div => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    self.stack.push(JsValue::Number(a / b));
                }
                OpCode::Mod => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    self.stack.push(JsValue::Number(a % b));
                }
                OpCode::Equal | OpCode::StrictEqual => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined);
                    self.stack.push(JsValue::Boolean(a == b));
                }
                OpCode::NotEqual | OpCode::StrictNotEqual => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined);
                    self.stack.push(JsValue::Boolean(a != b));
                }
                OpCode::Greater => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    self.stack.push(JsValue::Boolean(a > b));
                }
                OpCode::GreaterEqual => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    self.stack.push(JsValue::Boolean(a >= b));
                }
                OpCode::Less => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    self.stack.push(JsValue::Boolean(a < b));
                }
                OpCode::LessEqual => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    self.stack.push(JsValue::Boolean(a <= b));
                }
                OpCode::Not => {
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined);
                    self.stack.push(JsValue::Boolean(!a.is_truthy()));
                }
                OpCode::Negate => {
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined).to_number();
                    self.stack.push(JsValue::Number(-a));
                }
                OpCode::Jump(target) => {
                    frame.ip = target;
                }
                OpCode::JumpIfFalse(target) => {
                    let cond = self.stack.last().map(|v| v.is_truthy()).unwrap_or(false);
                    if !cond {
                        frame.ip = target;
                    }
                }
                OpCode::Loop(target) => {
                    frame.ip = target;
                }
                OpCode::Call(arg_count) => {
                    let mut args = Vec::new();
                    for _ in 0..arg_count {
                        args.push(self.stack.pop().unwrap_or(JsValue::Undefined));
                    }
                    args.reverse();

                    let callee = self.stack.pop().unwrap_or(JsValue::Undefined);
                    match callee {
                        JsValue::Function(f) => {
                            let stack_start = self.stack.len();
                            for arg in args {
                                self.stack.push(arg);
                            }
                            self.frames.push(CallFrame {
                                chunk_index: f.chunk_index,
                                ip: 0,
                                stack_start,
                            });
                        }
                        JsValue::NativeFunction(_, func) => {
                            let res = func(self, &args)?;
                            self.stack.push(res);
                        }
                        _ => return Err(format!("'{}' is not a function", callee.to_js_string())),
                    }
                }
                OpCode::Return => {
                    let ret = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let finished_frame = self.frames.pop().unwrap();
                    self.stack.truncate(finished_frame.stack_start);
                    self.stack.push(ret);
                    if self.frames.is_empty() {
                        return Ok(self.stack.pop().unwrap_or(JsValue::Undefined));
                    }
                }
                OpCode::GetProp(prop) => {
                    let obj_val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    match obj_val {
                        JsValue::Object(obj) => {
                            let val = obj.borrow().get(&prop);
                            self.stack.push(val);
                        }
                        JsValue::String(ref s) if prop == "length" => {
                            self.stack.push(JsValue::Number(s.chars().count() as f64));
                        }
                        _ => self.stack.push(JsValue::Undefined),
                    }
                }
                OpCode::SetProp(prop) => {
                    let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let obj_val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    if let JsValue::Object(obj) = obj_val {
                        obj.borrow_mut().set(prop, val);
                    }
                }
            }
        }

        Ok(self.stack.pop().unwrap_or(JsValue::Undefined))
    }
}
