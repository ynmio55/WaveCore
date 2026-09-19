use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use wavecore_dom::Node;

use crate::value::{JsObject, JsPromise, JsValue};
use crate::vm::VM;

static ELEMENT_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

pub struct DomBridge {
    pub root: Rc<RefCell<Node>>,
    pub listeners: Rc<RefCell<HashMap<(String, String), Vec<JsValue>>>>,
    pub current_url: Rc<RefCell<String>>,
    pub history_stack: Rc<RefCell<Vec<String>>>,
    pub timer_callbacks: Rc<RefCell<HashMap<usize, JsValue>>>,
}

impl DomBridge {
    pub fn new(root: Rc<RefCell<Node>>) -> Self {
        Self {
            root,
            listeners: Rc::new(RefCell::new(HashMap::new())),
            current_url: Rc::new(RefCell::new("https://wavecore.local/".to_string())),
            history_stack: Rc::new(RefCell::new(vec!["https://wavecore.local/".to_string()])),
            timer_callbacks: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn with_url(root: Rc<RefCell<Node>>, url: &str) -> Self {
        Self {
            root,
            listeners: Rc::new(RefCell::new(HashMap::new())),
            current_url: Rc::new(RefCell::new(url.to_string())),
            history_stack: Rc::new(RefCell::new(vec![url.to_string()])),
            timer_callbacks: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn attach_to_vm(&self, vm: &mut VM) {
        let root_ref = self.root.clone();
        let listeners_ref = self.listeners.clone();
        let url_ref = self.current_url.clone();
        let history_ref = self.history_stack.clone();

        // 1. document Object
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
                let mut borrowed = r2.borrow_mut();
                if let Some(node) = borrowed.query_selector_mut(&sel) {
                    let id = if let wavecore_dom::NodeType::Element(e) = &mut node.node_type {
                        if let Some(existing_id) = e.id() {
                            existing_id.to_string()
                        } else {
                            let gen_id = format!("_wc_auto_{}", ELEMENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst));
                            e.set_attribute("id", &gen_id);
                            gen_id
                        }
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

        // document.createElement(tagName)
        let r_create = root_ref.clone();
        let l_create = listeners_ref.clone();
        document.set(
            "createElement",
            JsValue::native("createElement", move |_vm, args| {
                let tag = args.first().map(|a| a.to_js_string()).unwrap_or_else(|| "div".to_string());
                let gen_id = format!("_wc_elem_{}", ELEMENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst));
                let mut new_node = Node::element(&tag, vec![]);
                if let wavecore_dom::NodeType::Element(e) = &mut new_node.node_type {
                    e.set_attribute("id", &gen_id);
                }
                // Store in root children temporarily as detached node
                r_create.borrow_mut().append_child(new_node);

                let elem = create_element_wrapper(&gen_id, r_create.clone(), l_create.clone());
                Ok(elem)
            }),
        );

        // document.title
        document.set("title", JsValue::String("WaveCore Browser".to_string()));

        let doc_val = JsValue::Object(Rc::new(RefCell::new(document)));
        vm.set_global("document", doc_val.clone());

        // 2. window.location Object
        let location_obj = create_location_object(url_ref.clone());
        let location_val = JsValue::Object(Rc::new(RefCell::new(location_obj)));
        vm.set_global("location", location_val.clone());

        // 3. window.history Object
        let history_obj = create_history_object(history_ref.clone());
        let history_val = JsValue::Object(Rc::new(RefCell::new(history_obj)));
        vm.set_global("history", history_val.clone());

        // 4. window Object
        let mut window = JsObject::new();
        window.set("document", doc_val);
        window.set("location", location_val);
        window.set("history", history_val);

        // window.alert
        window.set(
            "alert",
            JsValue::native("alert", |_vm, args| {
                let msg = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                println!("[Window Alert] {}", msg);
                Ok(JsValue::Undefined)
            }),
        );
        vm.set_global(
            "alert",
            JsValue::native("alert", |_vm, args| {
                let msg = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                println!("[Window Alert] {}", msg);
                Ok(JsValue::Undefined)
            }),
        );

        // window.setTimeout / clearTimeout
        let timers_ref = self.timer_callbacks.clone();
        let set_timeout_fn = JsValue::native("setTimeout", move |_vm, args| {
            if let Some(cb) = args.first().cloned() {
                let id = ELEMENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
                timers_ref.borrow_mut().insert(id, cb);
                Ok(JsValue::Number(id as f64))
            } else {
                Ok(JsValue::Number(0.0))
            }
        });
        window.set("setTimeout", set_timeout_fn.clone());
        vm.set_global("setTimeout", set_timeout_fn);

        let timers_clear = self.timer_callbacks.clone();
        let clear_timeout_fn = JsValue::native("clearTimeout", move |_vm, args| {
            let id = args.first().map(|a| a.to_number() as usize).unwrap_or(0);
            timers_clear.borrow_mut().remove(&id);
            Ok(JsValue::Undefined)
        });
        window.set("clearTimeout", clear_timeout_fn.clone());
        vm.set_global("clearTimeout", clear_timeout_fn);

        vm.set_global("window", JsValue::Object(Rc::new(RefCell::new(window))));

        // 5. URL API constructor
        vm.set_global(
            "URL",
            JsValue::native("URL", |_vm, args| {
                let url_str = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                let mut url_obj = JsObject::new();
                let parsed = parse_url_parts(&url_str);
                url_obj.set("href", JsValue::String(url_str.clone()));
                url_obj.set("protocol", JsValue::String(parsed.0));
                url_obj.set("host", JsValue::String(parsed.1));
                url_obj.set("pathname", JsValue::String(parsed.2));
                url_obj.set("search", JsValue::String(parsed.3));
                url_obj.set("hash", JsValue::String(parsed.4));
                Ok(JsValue::Object(Rc::new(RefCell::new(url_obj))))
            }),
        );

        // 6. Promise-based fetch(url)
        vm.set_global(
            "fetch",
            JsValue::native("fetch", |_vm, args| {
                let url = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                match wavecore_net::fetch_resource(&url) {
                    Ok(res) => {
                        let mut resp_obj = JsObject::new();
                        resp_obj.set("status", JsValue::Number(200.0));
                        resp_obj.set("ok", JsValue::Boolean(true));
                        resp_obj.set("url", JsValue::String(res.url.clone()));
                        resp_obj.set("contentType", JsValue::String(res.content_type.clone()));

                        let body_text = res.content.clone();
                        resp_obj.set(
                            "text",
                            JsValue::native("text", move |_vm, _args| {
                                Ok(JsValue::Promise(Rc::new(RefCell::new(
                                    JsPromise::resolved(JsValue::String(body_text.clone())),
                                ))))
                            }),
                        );

                        let body_json = res.content;
                        resp_obj.set(
                            "json",
                            JsValue::native("json", move |_vm, _args| {
                                Ok(JsValue::Promise(Rc::new(RefCell::new(
                                    JsPromise::resolved(JsValue::String(body_json.clone())),
                                ))))
                            }),
                        );

                        let response_val = JsValue::Object(Rc::new(RefCell::new(resp_obj)));
                        Ok(JsValue::Promise(Rc::new(RefCell::new(
                            JsPromise::resolved(response_val),
                        ))))
                    }
                    Err(e) => {
                        let err_val = JsValue::String(format!("fetch failed: {}", e));
                        Ok(JsValue::Promise(Rc::new(RefCell::new(
                            JsPromise::rejected(err_val),
                        ))))
                    }
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
                let _ = vm.call_function(&cb, &[]);
            }
        }
    }
}

fn parse_url_parts(url: &str) -> (String, String, String, String, String) {
    let protocol = if let Some(idx) = url.find("://") {
        url[..idx + 1].to_string()
    } else {
        "http:".to_string()
    };

    let rest = if let Some(idx) = url.find("://") {
        &url[idx + 3..]
    } else {
        url
    };

    let (host_part, path_part) = if let Some(idx) = rest.find('/') {
        (&rest[..idx], &rest[idx..])
    } else {
        (rest, "/")
    };

    let (pathname, search_part) = if let Some(idx) = path_part.find('?') {
        (&path_part[..idx], &path_part[idx..])
    } else {
        (path_part, "")
    };

    let (search, hash) = if let Some(idx) = search_part.find('#') {
        (&search_part[..idx], &search_part[idx..])
    } else {
        (search_part, "")
    };

    (protocol, host_part.to_string(), pathname.to_string(), search.to_string(), hash.to_string())
}

fn create_location_object(current_url: Rc<RefCell<String>>) -> JsObject {
    let mut loc = JsObject::new();
    let u = current_url.borrow().clone();
    let parsed = parse_url_parts(&u);

    loc.set("href", JsValue::String(u));
    loc.set("protocol", JsValue::String(parsed.0));
    loc.set("host", JsValue::String(parsed.1.clone()));
    loc.set("hostname", JsValue::String(parsed.1));
    loc.set("pathname", JsValue::String(parsed.2));
    loc.set("search", JsValue::String(parsed.3));
    loc.set("hash", JsValue::String(parsed.4));

    let u_assign = current_url.clone();
    loc.set(
        "assign",
        JsValue::native("assign", move |_vm, args| {
            let new_url = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            *u_assign.borrow_mut() = new_url;
            Ok(JsValue::Undefined)
        }),
    );

    let u_reload = current_url;
    loc.set(
        "reload",
        JsValue::native("reload", move |_vm, _args| {
            println!("[Location] Reloading page: {}", u_reload.borrow());
            Ok(JsValue::Undefined)
        }),
    );

    loc
}

fn create_history_object(history_stack: Rc<RefCell<Vec<String>>>) -> JsObject {
    let mut hist = JsObject::new();
    let len = history_stack.borrow().len();
    hist.set("length", JsValue::Number(len as f64));

    let h_back = history_stack.clone();
    hist.set(
        "back",
        JsValue::native("back", move |_vm, _args| {
            let mut stack = h_back.borrow_mut();
            if stack.len() > 1 {
                stack.pop();
            }
            Ok(JsValue::Undefined)
        }),
    );

    let h_forward = history_stack;
    hist.set(
        "forward",
        JsValue::native("forward", move |_vm, _args| {
            println!("[History] forward called (length: {})", h_forward.borrow().len());
            Ok(JsValue::Undefined)
        }),
    );

    hist
}

fn create_element_wrapper(
    id: &str,
    root: Rc<RefCell<Node>>,
    listeners: Rc<RefCell<HashMap<(String, String), Vec<JsValue>>>>,
) -> JsValue {
    let mut elem_obj = JsObject::new();
    let elem_id = id.to_string();

    elem_obj.set("id", JsValue::String(elem_id.clone()));

    // tagName
    let r_tag = root.clone();
    let id_tag = elem_id.clone();
    let tag = {
        let b = r_tag.borrow();
        b.find_by_id(&id_tag).and_then(|n| n.tag_name().map(|t| t.to_uppercase())).unwrap_or_default()
    };
    elem_obj.set("tagName", JsValue::String(tag));

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

    // getAttribute / setAttribute
    let r_getattr = root.clone();
    let id_getattr = elem_id.clone();
    elem_obj.set(
        "getAttribute",
        JsValue::native("getAttribute", move |_vm, args| {
            let attr = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let borrowed = r_getattr.borrow();
            if let Some(node) = borrowed.find_by_id(&id_getattr) {
                if let wavecore_dom::NodeType::Element(e) = &node.node_type {
                    if let Some(val) = e.get_attribute(&attr) {
                        return Ok(JsValue::String(val.to_string()));
                    }
                }
            }
            Ok(JsValue::Null)
        }),
    );

    let r_setattr = root.clone();
    let id_setattr = elem_id.clone();
    elem_obj.set(
        "setAttribute",
        JsValue::native("setAttribute", move |_vm, args| {
            let attr = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let val = args.get(1).map(|a| a.to_js_string()).unwrap_or_default();
            let mut borrowed = r_setattr.borrow_mut();
            if let Some(node) = borrowed.find_by_id_mut(&id_setattr) {
                if let wavecore_dom::NodeType::Element(e) = &mut node.node_type {
                    e.set_attribute(attr, val);
                }
            }
            Ok(JsValue::Undefined)
        }),
    );

    // classList Object: add, remove, toggle, contains
    let mut class_list = JsObject::new();
    let r_cl_add = root.clone();
    let id_cl_add = elem_id.clone();
    class_list.set(
        "add",
        JsValue::native("add", move |_vm, args| {
            let cls = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let mut borrowed = r_cl_add.borrow_mut();
            if let Some(node) = borrowed.find_by_id_mut(&id_cl_add) {
                node.add_class(&cls);
            }
            Ok(JsValue::Undefined)
        }),
    );

    let r_cl_rem = root.clone();
    let id_cl_rem = elem_id.clone();
    class_list.set(
        "remove",
        JsValue::native("remove", move |_vm, args| {
            let cls = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let mut borrowed = r_cl_rem.borrow_mut();
            if let Some(node) = borrowed.find_by_id_mut(&id_cl_rem) {
                node.remove_class(&cls);
            }
            Ok(JsValue::Undefined)
        }),
    );

    let r_cl_tog = root.clone();
    let id_cl_tog = elem_id.clone();
    class_list.set(
        "toggle",
        JsValue::native("toggle", move |_vm, args| {
            let cls = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let mut borrowed = r_cl_tog.borrow_mut();
            if let Some(node) = borrowed.find_by_id_mut(&id_cl_tog) {
                let res = node.toggle_class(&cls);
                return Ok(JsValue::Boolean(res));
            }
            Ok(JsValue::Boolean(false))
        }),
    );

    let r_cl_con = root.clone();
    let id_cl_con = elem_id.clone();
    class_list.set(
        "contains",
        JsValue::native("contains", move |_vm, args| {
            let cls = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let borrowed = r_cl_con.borrow();
            if let Some(node) = borrowed.find_by_id(&id_cl_con) {
                if let wavecore_dom::NodeType::Element(e) = &node.node_type {
                    return Ok(JsValue::Boolean(e.has_class(&cls)));
                }
            }
            Ok(JsValue::Boolean(false))
        }),
    );
    elem_obj.set("classList", JsValue::Object(Rc::new(RefCell::new(class_list))));

    // appendChild(child)
    let r_append = root.clone();
    let id_append = elem_id.clone();
    elem_obj.set(
        "appendChild",
        JsValue::native("appendChild", move |_vm, args| {
            if let Some(child_val) = args.first() {
                if let JsValue::Object(child_obj) = child_val {
                    let child_id = child_obj.borrow().get("id").to_js_string();
                    if !child_id.is_empty() {
                        let mut borrowed = r_append.borrow_mut();
                        // Find and clone or re-parent child node
                        if let Some(child_node) = borrowed.find_by_id(&child_id).cloned() {
                            if let Some(parent_node) = borrowed.find_by_id_mut(&id_append) {
                                parent_node.append_child(child_node);
                            }
                        }
                    }
                }
            }
            Ok(JsValue::Undefined)
        }),
    );

    // addEventListener
    let l_add = listeners;
    let id_evt = elem_id;
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

    // getContext(type) for canvas elements
    elem_obj.set(
        "getContext",
        JsValue::native("getContext", move |_vm, args| {
            let ctx_name = args.first().map(|a| a.to_js_string()).unwrap_or_else(|| "2d".to_string());
            if ctx_name == "2d" {
                Ok(JsValue::Object(Rc::new(RefCell::new(create_canvas_2d_context()))))
            } else if ctx_name == "webgl" || ctx_name == "experimental-webgl" {
                Ok(JsValue::Object(Rc::new(RefCell::new(create_webgl_context()))))
            } else {
                Ok(JsValue::Null)
            }
        }),
    );

    // Media element methods (play, pause)
    elem_obj.set(
        "play",
        JsValue::native("play", move |_vm, _args| {
            Ok(JsValue::Undefined)
        }),
    );
    elem_obj.set(
        "pause",
        JsValue::native("pause", move |_vm, _args| {
            Ok(JsValue::Undefined)
        }),
    );

    JsValue::Object(Rc::new(RefCell::new(elem_obj)))
}

fn create_canvas_2d_context() -> JsObject {
    let mut ctx = JsObject::new();
    ctx.set("fillStyle", JsValue::String("#000000".to_string()));
    ctx.set("strokeStyle", JsValue::String("#000000".to_string()));
    ctx.set("lineWidth", JsValue::Number(1.0));
    ctx.set("isCanvas2D", JsValue::Boolean(true));

    ctx.set("fillRect", JsValue::native("fillRect", |_vm, _args| Ok(JsValue::Undefined)));
    ctx.set("clearRect", JsValue::native("clearRect", |_vm, _args| Ok(JsValue::Undefined)));
    ctx.set("strokeRect", JsValue::native("strokeRect", |_vm, _args| Ok(JsValue::Undefined)));
    ctx.set("beginPath", JsValue::native("beginPath", |_vm, _args| Ok(JsValue::Undefined)));
    ctx.set("moveTo", JsValue::native("moveTo", |_vm, _args| Ok(JsValue::Undefined)));
    ctx.set("lineTo", JsValue::native("lineTo", |_vm, _args| Ok(JsValue::Undefined)));
    ctx.set("arc", JsValue::native("arc", |_vm, _args| Ok(JsValue::Undefined)));
    ctx.set("closePath", JsValue::native("closePath", |_vm, _args| Ok(JsValue::Undefined)));
    ctx.set("fill", JsValue::native("fill", |_vm, _args| Ok(JsValue::Undefined)));
    ctx.set("stroke", JsValue::native("stroke", |_vm, _args| Ok(JsValue::Undefined)));

    ctx
}

fn create_webgl_context() -> JsObject {
    let mut gl = JsObject::new();
    gl.set("isWebGL", JsValue::Boolean(true));
    gl.set("COLOR_BUFFER_BIT", JsValue::Number(16384.0));
    gl.set("DEPTH_BUFFER_BIT", JsValue::Number(256.0));
    gl.set("TRIANGLES", JsValue::Number(4.0));
    gl.set("ARRAY_BUFFER", JsValue::Number(34962.0));
    gl.set("STATIC_DRAW", JsValue::Number(35044.0));

    gl.set("viewport", JsValue::native("viewport", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("clearColor", JsValue::native("clearColor", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("clear", JsValue::native("clear", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("createBuffer", JsValue::native("createBuffer", |_vm, _args| {
        let mut buf = JsObject::new();
        buf.set("_webglBufferId", JsValue::Number(1.0));
        Ok(JsValue::Object(Rc::new(RefCell::new(buf))))
    }));
    gl.set("bindBuffer", JsValue::native("bindBuffer", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("bufferData", JsValue::native("bufferData", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("createShader", JsValue::native("createShader", |_vm, _args| {
        let mut s = JsObject::new();
        s.set("_webglShaderId", JsValue::Number(1.0));
        Ok(JsValue::Object(Rc::new(RefCell::new(s))))
    }));
    gl.set("shaderSource", JsValue::native("shaderSource", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("compileShader", JsValue::native("compileShader", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("createProgram", JsValue::native("createProgram", |_vm, _args| {
        let mut p = JsObject::new();
        p.set("_webglProgramId", JsValue::Number(1.0));
        Ok(JsValue::Object(Rc::new(RefCell::new(p))))
    }));
    gl.set("attachShader", JsValue::native("attachShader", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("linkProgram", JsValue::native("linkProgram", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("useProgram", JsValue::native("useProgram", |_vm, _args| Ok(JsValue::Undefined)));
    gl.set("drawArrays", JsValue::native("drawArrays", |_vm, _args| Ok(JsValue::Undefined)));

    gl
}
