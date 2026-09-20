use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::bytecode::{Chunk, OpCode};
use crate::value::{
    Environment, JsFunction, JsObject, JsPromise, JsValue, PromiseState,
};

pub struct CallFrame {
    pub chunk_index: usize,
    pub ip: usize,
    pub stack_start: usize,
    pub env: Rc<RefCell<Environment>>,
}

#[derive(Clone)]
pub struct TryHandler {
    pub catch_ip: usize,
    pub frame_depth: usize,
    pub stack_depth: usize,
    pub env: Rc<RefCell<Environment>>,
}

pub struct VM {
    pub chunks: Vec<Chunk>,
    pub stack: Vec<JsValue>,
    pub frames: Vec<CallFrame>,
    pub globals: HashMap<String, JsValue>,
    pub current_env: Rc<RefCell<Environment>>,
    pub try_stack: Vec<TryHandler>,
    pub microtasks: VecDeque<Rc<dyn Fn(&mut VM) -> Result<(), String>>>,
    pub instruction_limit: usize,
    pub instruction_count: usize,
    pub max_call_depth: usize,
    pub max_microtasks_per_checkpoint: usize,
    pub console_output: Vec<String>,
}

#[derive(Clone, Copy)]
enum TypedArrayKind {
    Float32,
    Uint8,
    Uint16,
    Uint32,
}

fn typed_array_number(value: f64, kind: TypedArrayKind) -> f64 {
    match kind {
        TypedArrayKind::Float32 => (value as f32) as f64,
        TypedArrayKind::Uint8 => (value as i128).rem_euclid(1i128 << 8) as f64,
        TypedArrayKind::Uint16 => (value as i128).rem_euclid(1i128 << 16) as f64,
        TypedArrayKind::Uint32 => (value as i128).rem_euclid(1i128 << 32) as f64,
    }
}

fn make_typed_array(source: Option<&JsValue>, kind: TypedArrayKind) -> JsValue {
    let values = match source {
        Some(JsValue::Array(items)) => items
            .borrow()
            .iter()
            .map(|value| JsValue::Number(typed_array_number(value.to_number(), kind)))
            .collect(),
        Some(JsValue::Number(length)) if length.is_finite() && *length >= 0.0 => {
            vec![JsValue::Number(0.0); (*length as usize).min(16_777_216)]
        }
        Some(other) => vec![JsValue::Number(typed_array_number(other.to_number(), kind))],
        None => Vec::new(),
    };
    JsValue::new_array(values)
}

fn json_to_js(value: &serde_json::Value) -> JsValue {
    match value {
        serde_json::Value::Null => JsValue::Null,
        serde_json::Value::Bool(v) => JsValue::Boolean(*v),
        serde_json::Value::Number(v) => JsValue::Number(v.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(v) => JsValue::String(v.clone()),
        serde_json::Value::Array(items) => {
            JsValue::new_array(items.iter().map(json_to_js).collect())
        }
        serde_json::Value::Object(map) => {
            let mut obj = JsObject::new();
            for (key, value) in map {
                obj.set(key.clone(), json_to_js(value));
            }
            JsValue::Object(Rc::new(RefCell::new(obj)))
        }
    }
}

fn js_to_json(value: &JsValue, depth: usize) -> Result<serde_json::Value, String> {
    if depth > 128 {
        return Err("JSON.stringify exceeded maximum nesting depth".to_string());
    }
    Ok(match value {
        JsValue::Undefined
        | JsValue::Function(_)
        | JsValue::NativeFunction(_, _)
        | JsValue::Promise(_) => serde_json::Value::Null,
        JsValue::Null => serde_json::Value::Null,
        JsValue::Boolean(v) => serde_json::Value::Bool(*v),
        JsValue::Number(v) => {
            if !v.is_finite() {
                serde_json::Value::Null
            } else {
                serde_json::Number::from_f64(*v)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            }
        }
        JsValue::String(v) => serde_json::Value::String(v.clone()),
        JsValue::Array(items) => serde_json::Value::Array(
            items
                .borrow()
                .iter()
                .map(|item| js_to_json(item, depth + 1))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        JsValue::Object(obj) => {
            let borrowed = obj.borrow();
            let mut map = serde_json::Map::new();
            for (key, value) in &borrowed.properties {
                if matches!(
                    value,
                    JsValue::Undefined | JsValue::Function(_) | JsValue::NativeFunction(_, _)
                ) {
                    continue;
                }
                map.insert(key.clone(), js_to_json(value, depth + 1)?);
            }
            serde_json::Value::Object(map)
        }
    })
}

impl VM {
    pub fn new() -> Self {
        let global_env = Rc::new(RefCell::new(Environment::new()));
        let mut vm = Self {
            chunks: Vec::new(),
            stack: Vec::new(),
            frames: Vec::new(),
            globals: HashMap::new(),
            current_env: global_env,
            try_stack: Vec::new(),
            microtasks: VecDeque::new(),
            instruction_limit: 1_000_000,
            instruction_count: 0,
            max_call_depth: 512,
            max_microtasks_per_checkpoint: 10_000,
            console_output: Vec::new(),
        };

        vm.init_builtins();
        vm
    }

    fn init_builtins(&mut self) {
        // console.log, console.error, console.warn
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
        console.set(
            "warn",
            JsValue::native("warn", |vm, args| {
                let text = args
                    .iter()
                    .map(|a| a.to_js_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("[JS console.warn] {}", text);
                vm.console_output.push(format!("WARN: {}", text));
                Ok(JsValue::Undefined)
            }),
        );
        self.globals.insert(
            "console".to_string(),
            JsValue::Object(Rc::new(RefCell::new(console))),
        );

        // Math built-in
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
        math.set(
            "round",
            JsValue::native("round", |_vm, args| {
                let val = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                Ok(JsValue::Number(val.round()))
            }),
        );
        math.set(
            "abs",
            JsValue::native("abs", |_vm, args| {
                let val = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                Ok(JsValue::Number(val.abs()))
            }),
        );
        math.set(
            "min",
            JsValue::native("min", |_vm, args| {
                let a = args.first().map(|v| v.to_number()).unwrap_or(f64::INFINITY);
                let b = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
                Ok(JsValue::Number(a.min(b)))
            }),
        );
        math.set(
            "max",
            JsValue::native("max", |_vm, args| {
                let a = args
                    .first()
                    .map(|v| v.to_number())
                    .unwrap_or(f64::NEG_INFINITY);
                let b = args
                    .get(1)
                    .map(|v| v.to_number())
                    .unwrap_or(f64::NEG_INFINITY);
                Ok(JsValue::Number(a.max(b)))
            }),
        );
        math.set(
            "random",
            JsValue::native("random", |_vm, _args| {
                // Pseudo-random value in [0, 1) using system clock
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(12345);
                let r = (nanos % 1_000_000) as f64 / 1_000_000.0;
                Ok(JsValue::Number(r))
            }),
        );
        math.set("PI", JsValue::Number(std::f64::consts::PI));
        math.set("E", JsValue::Number(std::f64::consts::E));
        self.globals.insert(
            "Math".to_string(),
            JsValue::Object(Rc::new(RefCell::new(math))),
        );

        // JSON built-in
        let mut json = JsObject::new();
        json.set(
            "stringify",
            JsValue::native("stringify", |_vm, args| {
                let val = args.first().cloned().unwrap_or(JsValue::Undefined);
                let json_value = js_to_json(&val, 0)?;
                serde_json::to_string(&json_value)
                    .map(JsValue::String)
                    .map_err(|e| format!("JSON.stringify failed: {e}"))
            }),
        );
        json.set(
            "parse",
            JsValue::native("parse", |_vm, args| {
                let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let parsed: serde_json::Value =
                    serde_json::from_str(&text).map_err(|e| format!("JSON.parse failed: {e}"))?;
                Ok(json_to_js(&parsed))
            }),
        );
        self.globals.insert(
            "JSON".to_string(),
            JsValue::Object(Rc::new(RefCell::new(json))),
        );

        // Array constructor & isArray
        let mut array_obj = JsObject::new();
        array_obj.set(
            "isArray",
            JsValue::native("isArray", |_vm, args| {
                let is_arr = matches!(args.first(), Some(JsValue::Array(_)));
                Ok(JsValue::Boolean(is_arr))
            }),
        );
        array_obj.set(
            "from",
            JsValue::native("from", |_vm, args| {
                let Some(source) = args.first() else {
                    return Ok(JsValue::new_array(Vec::new()));
                };
                match source {
                    JsValue::Array(items) => Ok(JsValue::new_array(items.borrow().clone())),
                    JsValue::String(text) => Ok(JsValue::new_array(
                        text.chars()
                            .map(|ch| JsValue::String(ch.to_string()))
                            .collect(),
                    )),
                    _ => Ok(JsValue::new_array(Vec::new())),
                }
            }),
        );
        self.globals.insert(
            "Array".to_string(),
            JsValue::Object(Rc::new(RefCell::new(array_obj))),
        );

        // Object helpers used by common framework/runtime code.
        let mut object_obj = JsObject::new();
        object_obj.set(
            "keys",
            JsValue::native("keys", |_vm, args| {
                let mut keys = match args.first() {
                    Some(JsValue::Object(obj)) => obj
                        .borrow()
                        .properties
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>(),
                    Some(JsValue::Array(items)) => (0..items.borrow().len())
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                keys.sort();
                Ok(JsValue::new_array(
                    keys.into_iter().map(JsValue::String).collect(),
                ))
            }),
        );
        object_obj.set(
            "assign",
            JsValue::native("assign", |_vm, args| {
                let target = args.first().cloned().unwrap_or_else(JsValue::new_object);
                let JsValue::Object(target_obj) = &target else {
                    return Ok(target);
                };
                for source in args.iter().skip(1) {
                    if let JsValue::Object(source_obj) = source {
                        let properties = source_obj.borrow().properties.clone();
                        for (key, value) in properties {
                            target_obj.borrow_mut().set(key, value);
                        }
                    }
                }
                Ok(target)
            }),
        );
        self.globals.insert(
            "Object".to_string(),
            JsValue::Object(Rc::new(RefCell::new(object_obj))),
        );

        // Typed arrays used heavily by graphics/media workloads. Pulse currently
        // stores them in the compact numeric array representation while preserving
        // constructor coercion semantics needed by WebGL buffer uploads.
        self.globals.insert(
            "Float32Array".to_string(),
            JsValue::native("Float32Array", |_vm, args| {
                Ok(make_typed_array(args.first(), TypedArrayKind::Float32))
            }),
        );
        self.globals.insert(
            "Uint8Array".to_string(),
            JsValue::native("Uint8Array", |_vm, args| {
                Ok(make_typed_array(args.first(), TypedArrayKind::Uint8))
            }),
        );
        self.globals.insert(
            "Uint16Array".to_string(),
            JsValue::native("Uint16Array", |_vm, args| {
                Ok(make_typed_array(args.first(), TypedArrayKind::Uint16))
            }),
        );
        self.globals.insert(
            "Uint32Array".to_string(),
            JsValue::native("Uint32Array", |_vm, args| {
                Ok(make_typed_array(args.first(), TypedArrayKind::Uint32))
            }),
        );

        // Promise built-in
        let mut promise_obj = JsObject::new();
        promise_obj.set(
            "resolve",
            JsValue::native("resolve", |_vm, args| {
                let val = args.first().cloned().unwrap_or(JsValue::Undefined);
                Ok(JsValue::Promise(Rc::new(RefCell::new(
                    JsPromise::resolved(val),
                ))))
            }),
        );
        promise_obj.set(
            "reject",
            JsValue::native("reject", |_vm, args| {
                let err = args.first().cloned().unwrap_or(JsValue::Undefined);
                Ok(JsValue::Promise(Rc::new(RefCell::new(
                    JsPromise::rejected(err),
                ))))
            }),
        );
        promise_obj.set(
            "all",
            JsValue::native("all", |_vm, args| {
                let Some(JsValue::Array(items)) = args.first() else {
                    return Ok(JsValue::Promise(Rc::new(RefCell::new(
                        JsPromise::resolved(JsValue::new_array(Vec::new())),
                    ))));
                };
                let mut values = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    match item {
                        JsValue::Promise(promise) => match &promise.borrow().state {
                            PromiseState::Fulfilled(value) => values.push(value.clone()),
                            PromiseState::Rejected(err) => {
                                return Ok(JsValue::Promise(Rc::new(RefCell::new(
                                    JsPromise::rejected(err.clone()),
                                ))));
                            }
                            PromiseState::Pending => {
                                return Ok(JsValue::Promise(Rc::new(RefCell::new(
                                    JsPromise::pending(),
                                ))));
                            }
                        },
                        other => values.push(other.clone()),
                    }
                }
                Ok(JsValue::Promise(Rc::new(RefCell::new(
                    JsPromise::resolved(JsValue::new_array(values)),
                ))))
            }),
        );
        promise_obj.set(
            "race",
            JsValue::native("race", |_vm, args| {
                let Some(JsValue::Array(items)) = args.first() else {
                    return Ok(JsValue::Promise(Rc::new(RefCell::new(JsPromise::pending()))));
                };
                for item in items.borrow().iter() {
                    match item {
                        JsValue::Promise(promise) => match &promise.borrow().state {
                            PromiseState::Fulfilled(value) => {
                                return Ok(JsValue::Promise(Rc::new(RefCell::new(
                                    JsPromise::resolved(value.clone()),
                                ))));
                            }
                            PromiseState::Rejected(err) => {
                                return Ok(JsValue::Promise(Rc::new(RefCell::new(
                                    JsPromise::rejected(err.clone()),
                                ))));
                            }
                            PromiseState::Pending => {}
                        },
                        other => {
                            return Ok(JsValue::Promise(Rc::new(RefCell::new(
                                JsPromise::resolved(other.clone()),
                            ))));
                        }
                    }
                }
                Ok(JsValue::Promise(Rc::new(RefCell::new(JsPromise::pending()))))
            }),
        );
        self.globals.insert(
            "Promise".to_string(),
            JsValue::Object(Rc::new(RefCell::new(promise_obj))),
        );

        // parseInt & parseFloat
        self.globals.insert(
            "parseInt".to_string(),
            JsValue::native("parseInt", |_vm, args| {
                let s = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                let n = s
                    .trim()
                    .parse::<i64>()
                    .map(|v| v as f64)
                    .unwrap_or(f64::NAN);
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

    pub fn queue_microtask(&mut self, task: impl Fn(&mut VM) -> Result<(), String> + 'static) {
        self.microtasks.push_back(Rc::new(task));
    }

    pub fn drain_microtasks(&mut self) -> Result<(), String> {
        let mut processed = 0usize;
        while let Some(task) = self.microtasks.pop_front() {
            processed += 1;
            if processed > self.max_microtasks_per_checkpoint {
                self.microtasks.clear();
                return Err("Execution terminated: microtask checkpoint exceeded configured limit".to_string());
            }
            task(self)?;
        }
        Ok(())
    }

    pub fn call_function(
        &mut self,
        callee: &JsValue,
        args: &[JsValue],
    ) -> Result<JsValue, String> {
        // Microtasks run at the end of the current JS turn, not after every nested
        // function/native call. External callbacks (timers/rAF) start with no active
        // VM frame, so they still flush their microtasks before returning to the host.
        let should_drain_microtasks = self.frames.is_empty();
        if matches!(callee, JsValue::Function(_)) && self.frames.len() >= self.max_call_depth {
            return Err(format!(
                "RangeError: maximum call stack size exceeded (limit {})",
                self.max_call_depth
            ));
        }
        match callee {
            JsValue::Function(f) => {
                let env = match &f.closure_env {
                    Some(parent) => {
                        Rc::new(RefCell::new(Environment::with_parent(parent.clone())))
                    }
                    None => Rc::new(RefCell::new(Environment::with_parent(
                        self.current_env.clone(),
                    ))),
                };

                for (i, param) in f.params.iter().enumerate() {
                    let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
                    env.borrow_mut().define(param.clone(), val);
                }

                let prev_env = self.current_env.clone();
                self.current_env = env;

                let target_depth = self.frames.len();
                let stack_start = self.stack.len();
                self.frames.push(CallFrame {
                    chunk_index: f.chunk_index,
                    ip: 0,
                    stack_start,
                    env: prev_env,
                });

                let res = self.run_until(target_depth)?;
                if should_drain_microtasks {
                    self.drain_microtasks()?;
                }
                Ok(res)
            }
            JsValue::NativeFunction(_, func) => {
                let res = func(self, args)?;
                if should_drain_microtasks {
                    self.drain_microtasks()?;
                }
                Ok(res)
            }
            _ => Err(format!("'{}' is not callable", callee.to_js_string())),
        }
    }

    pub fn execute(&mut self, chunks: Vec<Chunk>) -> Result<JsValue, String> {
        self.chunks = chunks;
        self.stack.clear();
        self.frames.clear();
        self.try_stack.clear();
        self.instruction_count = 0;

        let frame_env = self.current_env.clone();
        self.frames.push(CallFrame {
            chunk_index: 0,
            ip: 0,
            stack_start: 0,
            env: frame_env,
        });

        let result = self.run_until(0);
        let _ = self.drain_microtasks();
        result
    }

    fn unwind_exception(&mut self, err_val: JsValue) -> Result<(), String> {
        if let Some(handler) = self.try_stack.pop() {
            self.frames.truncate(handler.frame_depth);
            self.stack.truncate(handler.stack_depth);
            self.current_env = handler.env;
            self.stack.push(err_val);
            if let Some(top_frame) = self.frames.last_mut() {
                top_frame.ip = handler.catch_ip;
            }
            Ok(())
        } else {
            Err(format!("Uncaught exception: {}", err_val.to_js_string()))
        }
    }

    fn run_until(&mut self, target_depth: usize) -> Result<JsValue, String> {
        while self.frames.len() > target_depth {
            let frame = self.frames.last_mut().unwrap();
            self.instruction_count += 1;
            if self.instruction_count > self.instruction_limit {
                return Err("Execution terminated: Script exceeded maximum instruction limit (DoS protection)".into());
            }

            let chunk = &self.chunks[frame.chunk_index];
            if frame.ip >= chunk.code.len() {
                let finished_frame = self.frames.pop().unwrap();
                self.current_env = finished_frame.env;
                self.stack.truncate(finished_frame.stack_start);
                self.stack.push(JsValue::Undefined);
                if self.frames.len() == target_depth {
                    return Ok(self.stack.pop().unwrap_or(JsValue::Undefined));
                }
                continue;
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
                OpCode::DeclVar(name) => {
                    let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    self.current_env.borrow_mut().define(name, val);
                }
                OpCode::GetVar(name) => {
                    let val = self
                        .current_env
                        .borrow()
                        .get(&name)
                        .or_else(|| self.globals.get(&name).cloned())
                        .unwrap_or(JsValue::Undefined);
                    self.stack.push(val);
                }
                OpCode::SetVar(name) => {
                    let val = self.stack.last().cloned().unwrap_or(JsValue::Undefined);
                    let found = self.current_env.borrow_mut().set(&name, val.clone());
                    if !found {
                        self.globals.insert(name, val);
                    }
                }
                OpCode::GetGlobal(name) => {
                    let val = self.globals.get(&name).cloned().unwrap_or(JsValue::Undefined);
                    self.stack.push(val);
                }
                OpCode::SetGlobal(name) => {
                    let val = self.stack.last().cloned().unwrap_or(JsValue::Undefined);
                    self.globals.insert(name, val);
                }
                OpCode::Add => {
                    let b = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined);
                    match (&a, &b) {
                        (JsValue::String(s1), _) => {
                            self.stack
                                .push(JsValue::String(format!("{}{}", s1, b.to_js_string())));
                        }
                        (_, JsValue::String(s2)) => {
                            self.stack
                                .push(JsValue::String(format!("{}{}", a.to_js_string(), s2)));
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
                OpCode::TypeOf => {
                    let a = self.stack.pop().unwrap_or(JsValue::Undefined);
                    self.stack.push(JsValue::String(a.type_of().to_string()));
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
                            let env = match &f.closure_env {
                                Some(parent) => {
                                    Rc::new(RefCell::new(Environment::with_parent(parent.clone())))
                                }
                                None => Rc::new(RefCell::new(Environment::with_parent(
                                    self.current_env.clone(),
                                ))),
                            };

                            for (i, param) in f.params.iter().enumerate() {
                                let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
                                env.borrow_mut().define(param.clone(), val);
                            }

                            let prev_env = self.current_env.clone();
                            self.current_env = env;

                            let stack_start = self.stack.len();
                            self.frames.push(CallFrame {
                                chunk_index: f.chunk_index,
                                ip: 0,
                                stack_start,
                                env: prev_env,
                            });
                        }
                        JsValue::NativeFunction(_, func) => {
                            match func(self, &args) {
                                Ok(res) => self.stack.push(res),
                                Err(err_msg) => {
                                    self.unwind_exception(JsValue::String(err_msg))?;
                                }
                            }
                        }
                        _ => {
                            self.unwind_exception(JsValue::String(format!(
                                "'{}' is not a function",
                                callee.to_js_string()
                            )))?;
                        }
                    }
                }
                OpCode::Return => {
                    let ret = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let finished_frame = self.frames.pop().unwrap();
                    self.current_env = finished_frame.env;
                    self.stack.truncate(finished_frame.stack_start);
                    self.stack.push(ret);
                    if self.frames.len() == target_depth {
                        return Ok(self.stack.pop().unwrap_or(JsValue::Undefined));
                    }
                }
                OpCode::CreateArray(count) => {
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count {
                        items.push(self.stack.pop().unwrap_or(JsValue::Undefined));
                    }
                    items.reverse();
                    self.stack.push(JsValue::new_array(items));
                }
                OpCode::CreateObject(count) => {
                    let mut obj = JsObject::new();
                    let mut pairs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                        let key = self.stack.pop().unwrap_or(JsValue::Undefined).to_js_string();
                        pairs.push((key, val));
                    }
                    pairs.reverse();
                    for (k, v) in pairs {
                        obj.set(k, v);
                    }
                    self.stack.push(JsValue::Object(Rc::new(RefCell::new(obj))));
                }
                OpCode::MakeClosure {
                    chunk_index,
                    name,
                    params,
                } => {
                    let func = JsFunction {
                        name,
                        params,
                        chunk_index,
                        closure_env: Some(self.current_env.clone()),
                    };
                    self.stack.push(JsValue::Function(Rc::new(func)));
                }
                OpCode::GetIndex => {
                    let index_val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let obj_val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    match obj_val {
                        JsValue::Array(arr) => {
                            let idx = index_val.to_number() as usize;
                            let val = arr
                                .borrow()
                                .get(idx)
                                .cloned()
                                .unwrap_or(JsValue::Undefined);
                            self.stack.push(val);
                        }
                        JsValue::Object(obj) => {
                            let key = index_val.to_js_string();
                            self.stack.push(obj.borrow().get(&key));
                        }
                        JsValue::String(s) => {
                            let idx = index_val.to_number() as usize;
                            let ch = s
                                .chars()
                                .nth(idx)
                                .map(|c| JsValue::String(c.to_string()))
                                .unwrap_or(JsValue::Undefined);
                            self.stack.push(ch);
                        }
                        _ => self.stack.push(JsValue::Undefined),
                    }
                }
                OpCode::SetIndex => {
                    let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let index_val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    let obj_val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    match obj_val {
                        JsValue::Array(arr) => {
                            let idx = index_val.to_number() as usize;
                            let mut borrowed = arr.borrow_mut();
                            if idx >= borrowed.len() {
                                borrowed.resize(idx + 1, JsValue::Undefined);
                            }
                            borrowed[idx] = val;
                        }
                        JsValue::Object(obj) => {
                            let key = index_val.to_js_string();
                            obj.borrow_mut().set(key, val);
                        }
                        _ => {}
                    }
                }
                OpCode::GetProp(prop) => {
                    let obj_val = self.stack.pop().unwrap_or(JsValue::Undefined);
                    match obj_val {
                        JsValue::Object(obj) => {
                            let val = obj.borrow().get(&prop);
                            self.stack.push(val);
                        }
                        JsValue::Array(arr) => {
                            match prop.as_str() {
                                "length" => {
                                    self.stack
                                        .push(JsValue::Number(arr.borrow().len() as f64));
                                }
                                "push" => {
                                    let a_ref = arr.clone();
                                    self.stack.push(JsValue::native("push", move |_vm, args| {
                                        let mut b = a_ref.borrow_mut();
                                        for arg in args {
                                            b.push(arg.clone());
                                        }
                                        Ok(JsValue::Number(b.len() as f64))
                                    }));
                                }
                                "pop" => {
                                    let a_ref = arr.clone();
                                    self.stack.push(JsValue::native("pop", move |_vm, _args| {
                                        let mut b = a_ref.borrow_mut();
                                        Ok(b.pop().unwrap_or(JsValue::Undefined))
                                    }));
                                }
                                "join" => {
                                    let a_ref = arr.clone();
                                    self.stack.push(JsValue::native("join", move |_vm, args| {
                                        let sep = args
                                            .first()
                                            .map(|s| s.to_js_string())
                                            .unwrap_or_else(|| ",".to_string());
                                        let items: Vec<String> = a_ref
                                            .borrow()
                                            .iter()
                                            .map(|v| v.to_js_string())
                                            .collect();
                                        Ok(JsValue::String(items.join(&sep)))
                                    }));
                                }
                                "indexOf" => {
                                    let a_ref = arr.clone();
                                    self.stack.push(JsValue::native("indexOf", move |_vm, args| {
                                        let target = args.first().cloned().unwrap_or(JsValue::Undefined);
                                        let b = a_ref.borrow();
                                        for (i, v) in b.iter().enumerate() {
                                            if v == &target {
                                                return Ok(JsValue::Number(i as f64));
                                            }
                                        }
                                        Ok(JsValue::Number(-1.0))
                                    }));
                                }
                                "map" => {
                                    let a_ref = arr.clone();
                                    self.stack.push(JsValue::native("map", move |vm, args| {
                                        let callback = match args.first() {
                                            Some(cb) => cb.clone(),
                                            None => return Ok(JsValue::new_array(Vec::new())),
                                        };
                                        let items = a_ref.borrow().clone();
                                        let mut result = Vec::with_capacity(items.len());
                                        for (i, item) in items.iter().enumerate() {
                                            let res = vm.call_function(
                                                &callback,
                                                &[item.clone(), JsValue::Number(i as f64)],
                                            )?;
                                            result.push(res);
                                        }
                                        Ok(JsValue::new_array(result))
                                    }));
                                }
                                "forEach" => {
                                    let a_ref = arr.clone();
                                    self.stack.push(JsValue::native("forEach", move |vm, args| {
                                        if let Some(cb) = args.first().cloned() {
                                            let items = a_ref.borrow().clone();
                                            for (i, item) in items.iter().enumerate() {
                                                vm.call_function(
                                                    &cb,
                                                    &[item.clone(), JsValue::Number(i as f64)],
                                                )?;
                                            }
                                        }
                                        Ok(JsValue::Undefined)
                                    }));
                                }
                                "filter" => {
                                    let a_ref = arr.clone();
                                    self.stack.push(JsValue::native("filter", move |vm, args| {
                                        let Some(callback) = args.first().cloned() else {
                                            return Ok(JsValue::new_array(Vec::new()));
                                        };
                                        let items = a_ref.borrow().clone();
                                        let mut result = Vec::new();
                                        for (i, item) in items.iter().enumerate() {
                                            let keep = vm.call_function(
                                                &callback,
                                                &[item.clone(), JsValue::Number(i as f64)],
                                            )?;
                                            if keep.is_truthy() {
                                                result.push(item.clone());
                                            }
                                        }
                                        Ok(JsValue::new_array(result))
                                    }));
                                }
                                "reduce" => {
                                    let a_ref = arr.clone();
                                    self.stack.push(JsValue::native("reduce", move |vm, args| {
                                        let Some(callback) = args.first().cloned() else {
                                            return Ok(JsValue::Undefined);
                                        };
                                        let items = a_ref.borrow().clone();
                                        if items.is_empty() && args.get(1).is_none() {
                                            return Err("TypeError: reduce of empty array with no initial value".to_string());
                                        }
                                        let mut index = 0usize;
                                        let mut accumulator = if let Some(initial) = args.get(1) {
                                            initial.clone()
                                        } else {
                                            index = 1;
                                            items.first().cloned().unwrap_or(JsValue::Undefined)
                                        };
                                        while index < items.len() {
                                            accumulator = vm.call_function(
                                                &callback,
                                                &[
                                                    accumulator,
                                                    items[index].clone(),
                                                    JsValue::Number(index as f64),
                                                ],
                                            )?;
                                            index += 1;
                                        }
                                        Ok(accumulator)
                                    }));
                                }
                                "slice" => {
                                    let a_ref = arr.clone();
                                    self.stack.push(JsValue::native("slice", move |_vm, args| {
                                        let items = a_ref.borrow();
                                        let len = items.len() as isize;
                                        let normalize = |value: Option<&JsValue>, default: isize| {
                                            let raw = value.map(|v| v.to_number() as isize).unwrap_or(default);
                                            if raw < 0 { (len + raw).max(0) } else { raw.min(len) }
                                        };
                                        let start = normalize(args.get(0), 0) as usize;
                                        let end = normalize(args.get(1), len) as usize;
                                        let end = end.max(start).min(items.len());
                                        Ok(JsValue::new_array(items[start..end].to_vec()))
                                    }));
                                }
                                _ => self.stack.push(JsValue::Undefined),
                            }
                        }
                        JsValue::String(ref s) => {
                            match prop.as_str() {
                                "length" => {
                                    self.stack
                                        .push(JsValue::Number(s.chars().count() as f64));
                                }
                                "toLowerCase" => {
                                    let str_val = s.clone();
                                    self.stack.push(JsValue::native("toLowerCase", move |_vm, _args| {
                                        Ok(JsValue::String(str_val.to_lowercase()))
                                    }));
                                }
                                "toUpperCase" => {
                                    let str_val = s.clone();
                                    self.stack.push(JsValue::native("toUpperCase", move |_vm, _args| {
                                        Ok(JsValue::String(str_val.to_uppercase()))
                                    }));
                                }
                                "trim" => {
                                    let str_val = s.clone();
                                    self.stack.push(JsValue::native("trim", move |_vm, _args| {
                                        Ok(JsValue::String(str_val.trim().to_string()))
                                    }));
                                }
                                "split" => {
                                    let str_val = s.clone();
                                    self.stack.push(JsValue::native("split", move |_vm, args| {
                                        let sep = args
                                            .first()
                                            .map(|v| v.to_js_string())
                                            .unwrap_or_default();
                                        let parts: Vec<JsValue> = if sep.is_empty() {
                                            str_val.chars().map(|c| JsValue::String(c.to_string())).collect()
                                        } else {
                                            str_val.split(&sep).map(|p| JsValue::String(p.to_string())).collect()
                                        };
                                        Ok(JsValue::new_array(parts))
                                    }));
                                }
                                "indexOf" => {
                                    let str_val = s.clone();
                                    self.stack.push(JsValue::native("indexOf", move |_vm, args| {
                                        let needle = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                                        let idx = str_val.find(&needle).map(|i| i as f64).unwrap_or(-1.0);
                                        Ok(JsValue::Number(idx))
                                    }));
                                }
                                "includes" => {
                                    let str_val = s.clone();
                                    self.stack.push(JsValue::native("includes", move |_vm, args| {
                                        let needle = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                                        Ok(JsValue::Boolean(str_val.contains(&needle)))
                                    }));
                                }
                                "startsWith" => {
                                    let str_val = s.clone();
                                    self.stack.push(JsValue::native("startsWith", move |_vm, args| {
                                        let needle = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                                        Ok(JsValue::Boolean(str_val.starts_with(&needle)))
                                    }));
                                }
                                "endsWith" => {
                                    let str_val = s.clone();
                                    self.stack.push(JsValue::native("endsWith", move |_vm, args| {
                                        let needle = args.first().map(|v| v.to_js_string()).unwrap_or_default();
                                        Ok(JsValue::Boolean(str_val.ends_with(&needle)))
                                    }));
                                }
                                "replace" => {
                                    let str_val = s.clone();
                                    self.stack.push(JsValue::native("replace", move |_vm, args| {
                                        let from = args.get(0).map(|v| v.to_js_string()).unwrap_or_default();
                                        let to = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();
                                        Ok(JsValue::String(str_val.replacen(&from, &to, 1)))
                                    }));
                                }
                                _ => self.stack.push(JsValue::Undefined),
                            }
                        }
                        JsValue::Promise(promise) => {
                            match prop.as_str() {
                                "then" => {
                                    let p = promise.clone();
                                    self.stack.push(JsValue::native("then", move |vm, args| {
                                        let on_fulfilled = args.first().cloned();
                                        let on_rejected = args.get(1).cloned();
                                        let state = p.borrow().state.clone();
                                        match state {
                                            PromiseState::Fulfilled(val) => {
                                                if let Some(cb) = on_fulfilled {
                                                    let next_val = vm.call_function(&cb, &[val])?;
                                                    let unwrapped = if let JsValue::Promise(inner_p) = next_val {
                                                        match &inner_p.borrow().state {
                                                            PromiseState::Fulfilled(v) => v.clone(),
                                                            PromiseState::Rejected(err) => return Err(err.to_js_string()),
                                                            PromiseState::Pending => JsValue::Undefined,
                                                        }
                                                    } else {
                                                        next_val
                                                    };
                                                    Ok(JsValue::Promise(Rc::new(RefCell::new(
                                                        JsPromise::resolved(unwrapped),
                                                    ))))
                                                } else {
                                                    Ok(JsValue::Promise(Rc::new(RefCell::new(
                                                        JsPromise::resolved(val),
                                                    ))))
                                                }
                                            }
                                            PromiseState::Rejected(err) => {
                                                if let Some(cb) = on_rejected {
                                                    let handled = vm.call_function(&cb, &[err])?;
                                                    Ok(JsValue::Promise(Rc::new(RefCell::new(
                                                        JsPromise::resolved(handled),
                                                    ))))
                                                } else {
                                                    Ok(JsValue::Promise(Rc::new(RefCell::new(
                                                        JsPromise::rejected(err),
                                                    ))))
                                                }
                                            }
                                            PromiseState::Pending => {
                                                let new_p = Rc::new(RefCell::new(JsPromise::pending()));
                                                if let Some(cb) = on_fulfilled {
                                                    p.borrow_mut().then_callbacks.push((cb, on_rejected));
                                                }
                                                Ok(JsValue::Promise(new_p))
                                            }
                                        }
                                    }));
                                }
                                "catch" => {
                                    let p = promise.clone();
                                    self.stack.push(JsValue::native("catch", move |vm, args| {
                                        let on_rejected = args.first().cloned();
                                        let state = p.borrow().state.clone();
                                        match state {
                                            PromiseState::Rejected(err) => {
                                                if let Some(cb) = on_rejected {
                                                    let handled = vm.call_function(&cb, &[err])?;
                                                    Ok(JsValue::Promise(Rc::new(RefCell::new(
                                                        JsPromise::resolved(handled),
                                                    ))))
                                                } else {
                                                    Ok(JsValue::Promise(Rc::new(RefCell::new(
                                                        JsPromise::rejected(err),
                                                    ))))
                                                }
                                            }
                                            _ => Ok(JsValue::Promise(p.clone())),
                                        }
                                    }));
                                }
                                _ => self.stack.push(JsValue::Undefined),
                            }
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
                OpCode::PushTry { catch_ip } => {
                    self.try_stack.push(TryHandler {
                        catch_ip,
                        frame_depth: self.frames.len(),
                        stack_depth: self.stack.len(),
                        env: self.current_env.clone(),
                    });
                }
                OpCode::PopTry => {
                    self.try_stack.pop();
                }
                OpCode::Throw => {
                    let err = self.stack.pop().unwrap_or(JsValue::Undefined);
                    self.unwind_exception(err)?;
                }
            }
        }

        Ok(self.stack.pop().unwrap_or(JsValue::Undefined))
    }
}
