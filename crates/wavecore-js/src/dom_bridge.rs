use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wavecore_dom::Node;

use crate::value::{JsObject, JsValue};
use crate::vm::VM;

pub struct DomBridge {
    pub root: Rc<RefCell<Node>>,
    pub listeners: Rc<RefCell<HashMap<(String, String), Vec<JsValue>>>>, // (elem_id, event_name) -> callbacks
}

impl DomBridge {
    pub fn new(root: Rc<RefCell<Node>>) -> Self {
        Self {
            root,
            listeners: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn attach_to_vm(&self, vm: &mut VM) {
        let root_ref = self.root.clone();
        let listeners_ref = self.listeners.clone();

        let mut document = JsObject::new();

        // document.getElementById(id)
        let r1 = root_ref.clone();
        let l1 = listeners_ref.clone();
        document.set(
            "getElementById",
            JsValue::native("getElementById", move |_vm, args| {
                let id = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                let borrowed = r1.borrow();
                if let Some(_node) = borrowed.find_by_id(&id) {
                    let elem = create_element_wrapper(&id, r1.clone(), l1.clone());
                    Ok(elem)
                } else {
                    Ok(JsValue::Null)
                }
            }),
        );

        // document.querySelector(selector)
        let r2 = root_ref.clone();
        let l2 = listeners_ref.clone();
        document.set(
            "querySelector",
            JsValue::native("querySelector", move |_vm, args| {
                let sel = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                let borrowed = r2.borrow();
                if let Some(node) = borrowed.query_selector(&sel) {
                    let id = if let wavecore_dom::NodeType::Element(e) = &node.node_type {
                        e.id().unwrap_or("").to_string()
                    } else {
                        "".to_string()
                    };
                    let elem = create_element_wrapper(&id, r2.clone(), l2.clone());
                    Ok(elem)
                } else {
                    Ok(JsValue::Null)
                }
            }),
        );

        vm.set_global("document", JsValue::Object(Rc::new(RefCell::new(document))));

        // fetch(url)
        vm.set_global(
            "fetch",
            JsValue::native("fetch", |_vm, args| {
                let url = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                match wavecore_net::fetch_resource(&url) {
                    Ok(res) => {
                        let mut resp_obj = JsObject::new();
                        resp_obj.set("status", JsValue::Number(200.0));
                        resp_obj.set("ok", JsValue::Boolean(true));
                        resp_obj.set("url", JsValue::String(res.url));
                        resp_obj.set("contentType", JsValue::String(res.content_type));
                        resp_obj.set("body", JsValue::String(res.content));
                        Ok(JsValue::Object(Rc::new(RefCell::new(resp_obj))))
                    }
                    Err(e) => Err(format!("fetch failed: {}", e)),
                }
            }),
        );
    }

    pub fn dispatch_event(&self, vm: &mut VM, target_id: &str, event_name: &str) {
        let callbacks = {
            let map = self.listeners.borrow();
            map.get(&(target_id.to_string(), event_name.to_string())).cloned()
        };

        if let Some(cb_list) = callbacks {
            for cb in cb_list {
                match cb {
                    JsValue::Function(f) => {
                        let stack_start = vm.stack.len();
                        vm.frames.push(crate::vm::CallFrame {
                            chunk_index: f.chunk_index,
                            ip: 0,
                            stack_start,
                        });
                        let _ = vm.execute(vm.chunks.clone());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn create_element_wrapper(
    id: &str,
    root: Rc<RefCell<Node>>,
    listeners: Rc<RefCell<HashMap<(String, String), Vec<JsValue>>>>,
) -> JsValue {
    let mut elem_obj = JsObject::new();
    let elem_id = id.to_string();

    elem_obj.set("id", JsValue::String(elem_id.clone()));

    // innerText getter / setter
    let r = root.clone();
    let id_str = elem_id.clone();
    elem_obj.set(
        "getInnerText",
        JsValue::native("getInnerText", move |_vm, _args| {
            let borrowed = r.borrow();
            if let Some(node) = borrowed.find_by_id(&id_str) {
                Ok(JsValue::String(node.inner_text()))
            } else {
                Ok(JsValue::String(String::new()))
            }
        }),
    );

    let r_set = root.clone();
    let id_set = elem_id.clone();
    elem_obj.set(
        "setInnerText",
        JsValue::native("setInnerText", move |_vm, args| {
            let text = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let mut borrowed = r_set.borrow_mut();
            if let Some(node) = borrowed.find_by_id_mut(&id_set) {
                node.set_inner_text(&text);
            }
            Ok(JsValue::Undefined)
        }),
    );

    // value getter / setter for inputs
    let r_val = root.clone();
    let id_val = elem_id.clone();
    elem_obj.set(
        "getValue",
        JsValue::native("getValue", move |_vm, _args| {
            let borrowed = r_val.borrow();
            if let Some(node) = borrowed.find_by_id(&id_val) {
                if let wavecore_dom::NodeType::Element(e) = &node.node_type {
                    return Ok(JsValue::String(e.value().unwrap_or("").to_string()));
                }
            }
            Ok(JsValue::String(String::new()))
        }),
    );

    let r_setval = root.clone();
    let id_setval = elem_id.clone();
    elem_obj.set(
        "setValue",
        JsValue::native("setValue", move |_vm, args| {
            let val = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let mut borrowed = r_setval.borrow_mut();
            if let Some(node) = borrowed.find_by_id_mut(&id_setval) {
                if let wavecore_dom::NodeType::Element(e) = &mut node.node_type {
                    e.set_attribute("value", val);
                }
            }
            Ok(JsValue::Undefined)
        }),
    );

    // addEventListener
    let l_add = listeners.clone();
    let id_evt = elem_id.clone();
    elem_obj.set(
        "addEventListener",
        JsValue::native("addEventListener", move |_vm, args| {
            let evt = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            if let Some(cb) = args.get(1) {
                let mut map = l_add.borrow_mut();
                map.entry((id_evt.clone(), evt)).or_default().push(cb.clone());
            }
            Ok(JsValue::Undefined)
        }),
    );

    JsValue::Object(Rc::new(RefCell::new(elem_obj)))
}
