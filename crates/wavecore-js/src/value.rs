use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub type NativeFn = Rc<dyn Fn(&mut crate::vm::VM, &[JsValue]) -> Result<JsValue, String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedArrayKind {
    Float32,
    Uint8,
    Uint16,
    Uint32,
}

impl TypedArrayKind {
    pub fn bytes_per_element(self) -> usize {
        match self {
            Self::Float32 | Self::Uint32 => 4,
            Self::Uint16 => 2,
            Self::Uint8 => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Float32 => "Float32Array",
            Self::Uint8 => "Uint8Array",
            Self::Uint16 => "Uint16Array",
            Self::Uint32 => "Uint32Array",
        }
    }
}

#[derive(Clone)]
pub struct TypedArrayValue {
    pub buffer: Rc<RefCell<Vec<u8>>>,
    pub kind: TypedArrayKind,
    pub byte_offset: usize,
    pub length: usize,
}

impl TypedArrayValue {
    pub fn new(kind: TypedArrayKind, length: usize) -> Self {
        let bytes = length.saturating_mul(kind.bytes_per_element());
        Self {
            buffer: Rc::new(RefCell::new(vec![0; bytes])),
            kind,
            byte_offset: 0,
            length,
        }
    }

    pub fn from_buffer(
        buffer: Rc<RefCell<Vec<u8>>>,
        kind: TypedArrayKind,
        byte_offset: usize,
        length: Option<usize>,
    ) -> Result<Self, String> {
        let bpe = kind.bytes_per_element();
        if byte_offset % bpe != 0 {
            return Err("RangeError: typed array byteOffset must align to element size".to_string());
        }
        let buffer_len = buffer.borrow().len();
        if byte_offset > buffer_len {
            return Err("RangeError: typed array byteOffset exceeds ArrayBuffer".to_string());
        }
        let available = (buffer_len - byte_offset) / bpe;
        let length = length.unwrap_or(available);
        if length > available {
            return Err("RangeError: typed array length exceeds ArrayBuffer".to_string());
        }
        Ok(Self { buffer, kind, byte_offset, length })
    }

    pub fn byte_length(&self) -> usize {
        self.length.saturating_mul(self.kind.bytes_per_element())
    }

    pub fn get(&self, index: usize) -> Option<f64> {
        if index >= self.length {
            return None;
        }
        let start = self.byte_offset + index * self.kind.bytes_per_element();
        let bytes = self.buffer.borrow();
        Some(match self.kind {
            TypedArrayKind::Uint8 => bytes[start] as f64,
            TypedArrayKind::Uint16 => {
                u16::from_le_bytes([bytes[start], bytes[start + 1]]) as f64
            }
            TypedArrayKind::Uint32 => u32::from_le_bytes([
                bytes[start],
                bytes[start + 1],
                bytes[start + 2],
                bytes[start + 3],
            ]) as f64,
            TypedArrayKind::Float32 => f32::from_le_bytes([
                bytes[start],
                bytes[start + 1],
                bytes[start + 2],
                bytes[start + 3],
            ]) as f64,
        })
    }

    pub fn set(&mut self, index: usize, value: f64) {
        if index >= self.length {
            return;
        }
        let start = self.byte_offset + index * self.kind.bytes_per_element();
        let mut bytes = self.buffer.borrow_mut();
        match self.kind {
            TypedArrayKind::Uint8 => {
                bytes[start] = (value as i128).rem_euclid(1i128 << 8) as u8;
            }
            TypedArrayKind::Uint16 => {
                let raw = (value as i128).rem_euclid(1i128 << 16) as u16;
                bytes[start..start + 2].copy_from_slice(&raw.to_le_bytes());
            }
            TypedArrayKind::Uint32 => {
                let raw = (value as i128).rem_euclid(1i128 << 32) as u32;
                bytes[start..start + 4].copy_from_slice(&raw.to_le_bytes());
            }
            TypedArrayKind::Float32 => {
                bytes[start..start + 4].copy_from_slice(&(value as f32).to_le_bytes());
            }
        }
    }

    pub fn values(&self) -> Vec<JsValue> {
        (0..self.length)
            .map(|i| JsValue::Number(self.get(i).unwrap_or(0.0)))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled(JsValue),
    Rejected(JsValue),
}

#[derive(Clone)]
pub struct JsPromise {
    pub state: PromiseState,
    pub then_callbacks: Vec<(JsValue, Option<JsValue>)>, // (on_fulfilled, on_rejected)
}

impl JsPromise {
    pub fn resolved(val: JsValue) -> Self {
        Self {
            state: PromiseState::Fulfilled(val),
            then_callbacks: Vec::new(),
        }
    }

    pub fn rejected(err: JsValue) -> Self {
        Self {
            state: PromiseState::Rejected(err),
            then_callbacks: Vec::new(),
        }
    }

    pub fn pending() -> Self {
        Self {
            state: PromiseState::Pending,
            then_callbacks: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct Environment {
    pub parent: Option<Rc<RefCell<Environment>>>,
    pub bindings: HashMap<String, JsValue>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            parent: None,
            bindings: HashMap::new(),
        }
    }

    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Self {
        Self {
            parent: Some(parent),
            bindings: HashMap::new(),
        }
    }

    pub fn define(&mut self, name: impl Into<String>, val: JsValue) {
        self.bindings.insert(name.into(), val);
    }

    pub fn get(&self, name: &str) -> Option<JsValue> {
        if let Some(val) = self.bindings.get(name) {
            return Some(val.clone());
        }
        if let Some(parent) = &self.parent {
            return parent.borrow().get(name);
        }
        None
    }

    pub fn set(&mut self, name: &str, val: JsValue) -> bool {
        if self.bindings.contains_key(name) {
            self.bindings.insert(name.to_string(), val);
            return true;
        }
        if let Some(parent) = &self.parent {
            return parent.borrow_mut().set(name, val);
        }
        false
    }
}

#[derive(Clone)]
pub struct JsObject {
    pub properties: HashMap<String, JsValue>,
    pub proto: Option<Rc<RefCell<JsObject>>>,
}

impl JsObject {
    pub fn new() -> Self {
        Self {
            properties: HashMap::new(),
            proto: None,
        }
    }

    pub fn get(&self, key: &str) -> JsValue {
        if let Some(val) = self.properties.get(key) {
            return val.clone();
        }
        if let Some(proto) = &self.proto {
            return proto.borrow().get(key);
        }
        JsValue::Undefined
    }

    pub fn set(&mut self, key: impl Into<String>, val: JsValue) {
        self.properties.insert(key.into(), val);
    }
}

#[derive(Clone)]
pub struct JsFunction {
    pub name: String,
    pub params: Vec<String>,
    pub chunk_index: usize,
    pub closure_env: Option<Rc<RefCell<Environment>>>,
}

#[derive(Clone)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Rc<RefCell<Vec<JsValue>>>),
    ArrayBuffer(Rc<RefCell<Vec<u8>>>),
    TypedArray(Rc<RefCell<TypedArrayValue>>),
    Object(Rc<RefCell<JsObject>>),
    Function(Rc<JsFunction>),
    NativeFunction(String, NativeFn),
    Promise(Rc<RefCell<JsPromise>>),
}

impl JsValue {
    pub fn native(
        name: impl Into<String>,
        f: impl Fn(&mut crate::vm::VM, &[JsValue]) -> Result<JsValue, String> + 'static,
    ) -> Self {
        JsValue::NativeFunction(name.into(), Rc::new(f))
    }

    pub fn new_object() -> Self {
        JsValue::Object(Rc::new(RefCell::new(JsObject::new())))
    }

    pub fn new_array(items: Vec<JsValue>) -> Self {
        JsValue::Array(Rc::new(RefCell::new(items)))
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            JsValue::Undefined | JsValue::Null => false,
            JsValue::Boolean(b) => *b,
            JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
            JsValue::String(s) => !s.is_empty(),
            JsValue::Array(_)
            | JsValue::ArrayBuffer(_)
            | JsValue::TypedArray(_)
            | JsValue::Object(_)
            | JsValue::Function(_)
            | JsValue::NativeFunction(_, _)
            | JsValue::Promise(_) => true,
        }
    }

    pub fn type_of(&self) -> &'static str {
        match self {
            JsValue::Undefined => "undefined",
            JsValue::Null => "object", // Historical JS quirk
            JsValue::Boolean(_) => "boolean",
            JsValue::Number(_) => "number",
            JsValue::String(_) => "string",
            JsValue::Function(_) | JsValue::NativeFunction(_, _) => "function",
            JsValue::Array(_) | JsValue::ArrayBuffer(_) | JsValue::TypedArray(_) | JsValue::Object(_) | JsValue::Promise(_) => "object",
        }
    }

    pub fn to_js_string(&self) -> String {
        match self {
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Null => "null".to_string(),
            JsValue::Boolean(b) => b.to_string(),
            JsValue::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{:.0}", n)
                } else {
                    n.to_string()
                }
            }
            JsValue::String(s) => s.clone(),
            JsValue::Array(arr) => {
                let items: Vec<String> = arr.borrow().iter().map(|v| v.to_js_string()).collect();
                items.join(",")
            }
            JsValue::ArrayBuffer(_) => "[object ArrayBuffer]".to_string(),
            JsValue::TypedArray(array) => array
                .borrow()
                .values()
                .iter()
                .map(|v| v.to_js_string())
                .collect::<Vec<_>>()
                .join(","),
            JsValue::Object(obj) => {
                if let Some(t) = obj.borrow().properties.get("toString") {
                    if let JsValue::String(s) = t {
                        return s.clone();
                    }
                }
                "[object Object]".to_string()
            }
            JsValue::Function(f) => format!("function {}() {{ [bytecode] }}", f.name),
            JsValue::NativeFunction(name, _) => format!("function {}() {{ [native code] }}", name),
            JsValue::Promise(_) => "[object Promise]".to_string(),
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            JsValue::Undefined => f64::NAN,
            JsValue::Null => 0.0,
            JsValue::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            JsValue::Number(n) => *n,
            JsValue::String(s) => s.trim().parse().unwrap_or(f64::NAN),
            JsValue::Array(arr) => {
                let b = arr.borrow();
                if b.is_empty() {
                    0.0
                } else if b.len() == 1 {
                    b[0].to_number()
                } else {
                    f64::NAN
                }
            }
            JsValue::ArrayBuffer(_)
            | JsValue::TypedArray(_)
            | JsValue::Object(_)
            | JsValue::Function(_)
            | JsValue::NativeFunction(_, _)
            | JsValue::Promise(_) => f64::NAN,
        }
    }
}

impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsValue::Undefined, JsValue::Undefined) => true,
            (JsValue::Null, JsValue::Null) => true,
            (JsValue::Undefined, JsValue::Null) | (JsValue::Null, JsValue::Undefined) => true,
            (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
            (JsValue::Number(a), JsValue::Number(b)) => a == b,
            (JsValue::String(a), JsValue::String(b)) => a == b,
            (JsValue::Array(a), JsValue::Array(b)) => Rc::ptr_eq(a, b),
            (JsValue::ArrayBuffer(a), JsValue::ArrayBuffer(b)) => Rc::ptr_eq(a, b),
            (JsValue::TypedArray(a), JsValue::TypedArray(b)) => Rc::ptr_eq(a, b),
            (JsValue::Object(a), JsValue::Object(b)) => Rc::ptr_eq(a, b),
            (JsValue::Promise(a), JsValue::Promise(b)) => Rc::ptr_eq(a, b),
            (JsValue::Number(a), JsValue::String(b)) => {
                *a == b.trim().parse::<f64>().unwrap_or(f64::NAN)
            }
            (JsValue::String(a), JsValue::Number(b)) => {
                a.trim().parse::<f64>().unwrap_or(f64::NAN) == *b
            }
            _ => false,
        }
    }
}

impl fmt::Debug for JsValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_js_string())
    }
}
