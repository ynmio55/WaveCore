use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub type NativeFn = Rc<dyn Fn(&mut crate::vm::VM, &[JsValue]) -> Result<JsValue, String>>;

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
}

#[derive(Clone)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Object(Rc<RefCell<JsObject>>),
    Function(Rc<JsFunction>),
    NativeFunction(String, NativeFn),
}

impl JsValue {
    pub fn native(
        name: impl Into<String>,
        f: impl Fn(&mut crate::vm::VM, &[JsValue]) -> Result<JsValue, String> + 'static,
    ) -> Self {
        JsValue::NativeFunction(name.into(), Rc::new(f))
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            JsValue::Undefined | JsValue::Null => false,
            JsValue::Boolean(b) => *b,
            JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
            JsValue::String(s) => !s.is_empty(),
            JsValue::Object(_) | JsValue::Function(_) | JsValue::NativeFunction(_, _) => true,
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
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            JsValue::Undefined => f64::NAN,
            JsValue::Null => 0.0,
            JsValue::Boolean(b) => if *b { 1.0 } else { 0.0 },
            JsValue::Number(n) => *n,
            JsValue::String(s) => s.trim().parse().unwrap_or(f64::NAN),
            JsValue::Object(_) | JsValue::Function(_) | JsValue::NativeFunction(_, _) => f64::NAN,
        }
    }

    pub fn new_object() -> Self {
        JsValue::Object(Rc::new(RefCell::new(JsObject::new())))
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
            (JsValue::Number(a), JsValue::String(b)) => *a == b.trim().parse::<f64>().unwrap_or(f64::NAN),
            (JsValue::String(a), JsValue::Number(b)) => a.trim().parse::<f64>().unwrap_or(f64::NAN) == *b,
            (JsValue::Object(a), JsValue::Object(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl fmt::Debug for JsValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_js_string())
    }
}
