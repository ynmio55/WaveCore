use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub type NativeFn = Rc<dyn Fn(&mut crate::vm::VM, &[JsValue]) -> Result<JsValue, String>>;

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
            JsValue::Array(_) | JsValue::Object(_) | JsValue::Promise(_) => "object",
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
            JsValue::Object(_)
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
