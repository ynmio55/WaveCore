use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use wavecore_dom::Node;
use wavecore_net::NetworkClient;
use wavecore_sandbox::Origin;
use wavecore_render::{Canvas2DCommand, WebGlCommand};

use crate::value::{JsObject, JsPromise, JsValue};
use crate::vm::VM;

static ELEMENT_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Copy)]
enum TimerKind {
    Timeout,
    Interval(Duration),
    AnimationFrame,
}

#[derive(Clone)]
struct TimerEntry {
    due: Instant,
    callback: JsValue,
    kind: TimerKind,
}

#[derive(Clone, Copy)]
enum CanvasPathPrimitive {
    Line { x1: f32, y1: f32, x2: f32, y2: f32 },
    Circle { cx: f32, cy: f32, radius: f32 },
}

#[derive(Default)]
struct CanvasPathState {
    primitives: Vec<CanvasPathPrimitive>,
    current: Option<(f32, f32)>,
    first: Option<(f32, f32)>,
}

pub struct DomBridge {
    pub root: Rc<RefCell<Node>>,
    pub listeners: Rc<RefCell<HashMap<(String, String), Vec<JsValue>>>>,
    pub current_url: Rc<RefCell<String>>,
    pub history_stack: Rc<RefCell<Vec<String>>>,
    timer_callbacks: Rc<RefCell<HashMap<usize, TimerEntry>>>,
    time_origin: Rc<Instant>,
    network_client: Rc<RefCell<NetworkClient>>,
    canvas_commands: Rc<RefCell<HashMap<u64, Vec<Canvas2DCommand>>>>,
    webgl_commands: Rc<RefCell<HashMap<u64, Vec<WebGlCommand>>>>,
}

impl DomBridge {
    pub fn new(root: Rc<RefCell<Node>>) -> Self {
        Self {
            root,
            listeners: Rc::new(RefCell::new(HashMap::new())),
            current_url: Rc::new(RefCell::new("https://wavecore.local/".to_string())),
            history_stack: Rc::new(RefCell::new(vec!["https://wavecore.local/".to_string()])),
            timer_callbacks: Rc::new(RefCell::new(HashMap::new())),
            time_origin: Rc::new(Instant::now()),
            network_client: Rc::new(RefCell::new(NetworkClient::new())),
            canvas_commands: Rc::new(RefCell::new(HashMap::new())),
            webgl_commands: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn with_url(root: Rc<RefCell<Node>>, url: &str) -> Self {
        Self::with_url_and_client(root, url, Rc::new(RefCell::new(NetworkClient::new())))
    }

    pub fn with_url_and_client(
        root: Rc<RefCell<Node>>,
        url: &str,
        network_client: Rc<RefCell<NetworkClient>>,
    ) -> Self {
        Self {
            root,
            listeners: Rc::new(RefCell::new(HashMap::new())),
            current_url: Rc::new(RefCell::new(url.to_string())),
            history_stack: Rc::new(RefCell::new(vec![url.to_string()])),
            timer_callbacks: Rc::new(RefCell::new(HashMap::new())),
            time_origin: Rc::new(Instant::now()),
            network_client,
            canvas_commands: Rc::new(RefCell::new(HashMap::new())),
            webgl_commands: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn canvas_commands_snapshot(&self) -> HashMap<u64, Vec<Canvas2DCommand>> {
        self.canvas_commands.borrow().clone()
    }

    pub fn webgl_commands_snapshot(&self) -> HashMap<u64, Vec<WebGlCommand>> {
        self.webgl_commands.borrow().clone()
    }

    pub fn attach_to_vm(&self, vm: &mut VM) {
        let canvas_commands_ref = self.canvas_commands.clone();
        let webgl_commands_ref = self.webgl_commands.clone();
        let root_ref = self.root.clone();
        let listeners_ref = self.listeners.clone();
        let url_ref = self.current_url.clone();
        let history_ref = self.history_stack.clone();

        // 1. document Object
        let mut document = JsObject::new();

        // document.getElementById(id)
        let r1 = root_ref.clone();
        let l1 = listeners_ref.clone();
        let c1 = canvas_commands_ref.clone();
        let w1 = webgl_commands_ref.clone();
        document.set(
            "getElementById",
            JsValue::native("getElementById", move |_vm, args| {
                let id = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                let borrowed = r1.borrow();
                if let Some(_node) = borrowed.find_by_id(&id) {
                    let elem = create_element_wrapper(&id, r1.clone(), l1.clone(), c1.clone(), w1.clone());
                    Ok(elem)
                } else {
                    Ok(JsValue::Null)
                }
            }),
        );

        // document.querySelector(selector)
        let r2 = root_ref.clone();
        let l2 = listeners_ref.clone();
        let c2 = canvas_commands_ref.clone();
        let w2 = webgl_commands_ref.clone();
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
                    let elem = create_element_wrapper(&id, r2.clone(), l2.clone(), c2.clone(), w2.clone());
                    Ok(elem)
                } else {
                    Ok(JsValue::Null)
                }
            }),
        );

        // document.querySelectorAll(selector)
        let r_all = root_ref.clone();
        let l_all = listeners_ref.clone();
        let c_all = canvas_commands_ref.clone();
        let w_all = webgl_commands_ref.clone();
        document.set(
            "querySelectorAll",
            JsValue::native("querySelectorAll", move |_vm, args| {
                let sel = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                let ids = r_all.borrow().query_selector_all_ids(&sel);
                let mut wrapped = Vec::with_capacity(ids.len());

                for node_id in ids {
                    let html_id = {
                        let mut root = r_all.borrow_mut();
                        let Some(node) = root.find_by_node_id_mut(node_id) else {
                            continue;
                        };
                        let wavecore_dom::NodeType::Element(element) = &mut node.node_type else {
                            continue;
                        };
                        if let Some(existing) = element.id() {
                            existing.to_string()
                        } else {
                            let generated = format!(
                                "_wc_auto_{}",
                                ELEMENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
                            );
                            element.set_attribute("id", &generated);
                            generated
                        }
                    };
                    wrapped.push(create_element_wrapper(
                        &html_id,
                        r_all.clone(),
                        l_all.clone(),
                        c_all.clone(),
                        w_all.clone(),
                    ));
                }

                Ok(JsValue::new_array(wrapped))
            }),
        );

        // document.createElement(tagName)
        let r_create = root_ref.clone();
        let l_create = listeners_ref.clone();
        let c_create = canvas_commands_ref.clone();
        let w_create = webgl_commands_ref.clone();
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

                let elem = create_element_wrapper(&gen_id, r_create.clone(), l_create.clone(), c_create.clone(), w_create.clone());
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

        // High-resolution monotonic timing API.
        let perf_origin = self.time_origin.clone();
        let mut performance = JsObject::new();
        performance.set(
            "now",
            JsValue::native("now", move |_vm, _args| {
                Ok(JsValue::Number(perf_origin.elapsed().as_secs_f64() * 1000.0))
            }),
        );
        let performance_val = JsValue::Object(Rc::new(RefCell::new(performance)));
        window.set("performance", performance_val.clone());
        vm.set_global("performance", performance_val);

        // queueMicrotask(callback) schedules work after the current JS turn.
        let queue_microtask = JsValue::native("queueMicrotask", move |vm, args| {
            if let Some(callback) = args.first().cloned() {
                vm.queue_microtask(move |vm| {
                    vm.call_function(&callback, &[]).map(|_| ())
                });
            }
            Ok(JsValue::Undefined)
        });
        window.set("queueMicrotask", queue_microtask.clone());
        vm.set_global("queueMicrotask", queue_microtask);

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
                let delay_ms = args
                    .get(1)
                    .map(|v| v.to_number().max(0.0))
                    .unwrap_or(0.0)
                    .min(86_400_000.0) as u64;
                timers_ref.borrow_mut().insert(
                    id,
                    TimerEntry {
                        due: Instant::now() + Duration::from_millis(delay_ms),
                        callback: cb,
                        kind: TimerKind::Timeout,
                    },
                );
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

        let interval_timers = self.timer_callbacks.clone();
        let set_interval_fn = JsValue::native("setInterval", move |_vm, args| {
            if let Some(cb) = args.first().cloned() {
                let id = ELEMENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
                let delay_ms = args
                    .get(1)
                    .map(|v| v.to_number().max(1.0))
                    .unwrap_or(1.0)
                    .min(86_400_000.0) as u64;
                let period = Duration::from_millis(delay_ms);
                interval_timers.borrow_mut().insert(
                    id,
                    TimerEntry {
                        due: Instant::now() + period,
                        callback: cb,
                        kind: TimerKind::Interval(period),
                    },
                );
                Ok(JsValue::Number(id as f64))
            } else {
                Ok(JsValue::Number(0.0))
            }
        });
        window.set("setInterval", set_interval_fn.clone());
        vm.set_global("setInterval", set_interval_fn);

        let interval_clear = self.timer_callbacks.clone();
        let clear_interval_fn = JsValue::native("clearInterval", move |_vm, args| {
            let id = args.first().map(|a| a.to_number() as usize).unwrap_or(0);
            interval_clear.borrow_mut().remove(&id);
            Ok(JsValue::Undefined)
        });
        window.set("clearInterval", clear_interval_fn.clone());
        vm.set_global("clearInterval", clear_interval_fn);

        // requestAnimationFrame / cancelAnimationFrame. The browser event loop batches
        // callbacks on a ~60 Hz cadence; the callback receives a monotonic timestamp.
        let raf_timers = self.timer_callbacks.clone();
        let request_animation_frame = JsValue::native("requestAnimationFrame", move |_vm, args| {
            if let Some(cb) = args.first().cloned() {
                let id = ELEMENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
                raf_timers.borrow_mut().insert(
                    id,
                    TimerEntry {
                        due: Instant::now() + Duration::from_millis(16),
                        callback: cb,
                        kind: TimerKind::AnimationFrame,
                    },
                );
                Ok(JsValue::Number(id as f64))
            } else {
                Ok(JsValue::Number(0.0))
            }
        });
        window.set("requestAnimationFrame", request_animation_frame.clone());
        vm.set_global("requestAnimationFrame", request_animation_frame);

        let cancel_raf_timers = self.timer_callbacks.clone();
        let cancel_animation_frame = JsValue::native("cancelAnimationFrame", move |_vm, args| {
            let id = args.first().map(|a| a.to_number() as usize).unwrap_or(0);
            cancel_raf_timers.borrow_mut().remove(&id);
            Ok(JsValue::Undefined)
        });
        window.set("cancelAnimationFrame", cancel_animation_frame.clone());
        vm.set_global("cancelAnimationFrame", cancel_animation_frame);

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

        // 6. Promise-based fetch(url) with persistent per-page network state.
        // The same NetworkClient is reused so cookies and cache survive across fetch calls.
        let fetch_client = self.network_client.clone();
        let fetch_origin_url = self.current_url.clone();
        vm.set_global(
            "fetch",
            JsValue::native("fetch", move |_vm, args| {
                let url = args.first().map(|a| a.to_js_string()).unwrap_or_default();
                let caller_origin = Origin::parse(&fetch_origin_url.borrow()).ok();
                match fetch_client
                    .borrow_mut()
                    .fetch_with_origin(&url, caller_origin.as_ref())
                {
                    Ok(res) => {
                        let mut resp_obj = JsObject::new();
                        resp_obj.set("status", JsValue::Number(res.status_code as f64));
                        resp_obj.set("statusText", JsValue::String(res.status_text.clone()));
                        resp_obj.set("ok", JsValue::Boolean(res.is_ok()));
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
                            JsValue::native("json", move |vm, _args| {
                                let json_global = vm
                                    .get_global("JSON")
                                    .cloned()
                                    .ok_or_else(|| "JSON global is unavailable".to_string())?;
                                let JsValue::Object(json_obj) = json_global else {
                                    return Err("JSON global is invalid".to_string());
                                };
                                let parse = json_obj.borrow().get("parse");
                                let parsed = vm.call_function(
                                    &parse,
                                    &[JsValue::String(body_json.clone())],
                                )?;
                                Ok(JsValue::Promise(Rc::new(RefCell::new(
                                    JsPromise::resolved(parsed),
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

    pub fn dispatch_due_timers(&self, vm: &mut VM) -> usize {
        let now = Instant::now();
        let due_ids: Vec<usize> = self
            .timer_callbacks
            .borrow()
            .iter()
            .filter_map(|(id, entry)| (entry.due <= now).then_some(*id))
            .collect();

        let mut callbacks = Vec::with_capacity(due_ids.len());
        {
            let mut timers = self.timer_callbacks.borrow_mut();
            for id in due_ids {
                if let Some(entry) = timers.remove(&id) {
                    callbacks.push((id, entry));
                }
            }
        }

        let count = callbacks.len();
        let timestamp_ms = self.time_origin.elapsed().as_secs_f64() * 1000.0;
        for (id, entry) in callbacks {
            match entry.kind {
                TimerKind::Timeout => {
                    let _ = vm.call_function(&entry.callback, &[]);
                }
                TimerKind::Interval(period) => {
                    let _ = vm.call_function(&entry.callback, &[]);
                    self.timer_callbacks.borrow_mut().insert(
                        id,
                        TimerEntry {
                            due: Instant::now() + period,
                            callback: entry.callback,
                            kind: TimerKind::Interval(period),
                        },
                    );
                }
                TimerKind::AnimationFrame => {
                    let _ = vm.call_function(
                        &entry.callback,
                        &[JsValue::Number(timestamp_ms)],
                    );
                }
            }
        }
        count
    }

    pub fn dispatch_event(&self, vm: &mut VM, target_id: &str, event_name: &str) -> bool {
        self.dispatch_event_with_data(vm, target_id, event_name, None)
    }

    pub fn dispatch_event_with_data(
        &self,
        vm: &mut VM,
        target_id: &str,
        event_name: &str,
        data: Option<&str>,
    ) -> bool {
        let path_ids = {
            let root = self.root.borrow();
            let Some(target) = root.find_by_id(target_id) else {
                return false;
            };
            root.ancestor_ids_for(target.node_id()).unwrap_or_default()
        };

        let default_prevented = Rc::new(Cell::new(false));

        for node_id in path_ids.into_iter().rev() {
            let current_id = {
                let root = self.root.borrow();
                root.find_by_node_id(node_id)
                    .and_then(|node| match &node.node_type {
                        wavecore_dom::NodeType::Element(element) => {
                            element.id().map(str::to_string)
                        }
                        _ => None,
                    })
            };
            let Some(current_id) = current_id else {
                continue;
            };

            let callbacks = {
                let map = self.listeners.borrow();
                map.get(&(current_id.clone(), event_name.to_string()))
                    .cloned()
                    .unwrap_or_default()
            };

            for callback in callbacks {
                let mut event = JsObject::new();
                event.set("type", JsValue::String(event_name.to_string()));
                event.set("targetId", JsValue::String(target_id.to_string()));
                event.set("currentTargetId", JsValue::String(current_id.clone()));
                event.set("bubbles", JsValue::Boolean(true));
                if let Some(data) = data {
                    event.set("data", JsValue::String(data.to_string()));
                }

                let prevented = default_prevented.clone();
                event.set(
                    "preventDefault",
                    JsValue::native("preventDefault", move |_vm, _args| {
                        prevented.set(true);
                        Ok(JsValue::Undefined)
                    }),
                );

                let _ = vm.call_function(
                    &callback,
                    &[JsValue::Object(Rc::new(RefCell::new(event)))],
                );
            }
        }

        default_prevented.get()
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
    canvas_commands: Rc<RefCell<HashMap<u64, Vec<Canvas2DCommand>>>>,
    webgl_commands: Rc<RefCell<HashMap<u64, Vec<WebGlCommand>>>>,
) -> JsValue {
    let mut elem_obj = JsObject::new();
    let elem_id = id.to_string();

    elem_obj.set("id", JsValue::String(elem_id.clone()));
    let internal_node_id = root
        .borrow()
        .find_by_id(&elem_id)
        .map(|node| node.node_id().0)
        .unwrap_or(0);
    elem_obj.set("nodeId", JsValue::Number(internal_node_id as f64));

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

    // appendChild(child): re-parent the existing node instead of cloning it.
    let r_append = root.clone();
    let id_append = elem_id.clone();
    elem_obj.set(
        "appendChild",
        JsValue::native("appendChild", move |_vm, args| {
            if let Some(JsValue::Object(child_obj)) = args.first() {
                let child_id = child_obj.borrow().get("id").to_js_string();
                if !child_id.is_empty() && child_id != id_append {
                    let mut borrowed = r_append.borrow_mut();
                    let child_node_id = borrowed.find_by_id(&child_id).map(|node| node.node_id());
                    if let Some(child_node_id) = child_node_id {
                        if let Some(child_node) = borrowed.detach_by_id(child_node_id) {
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

    // remove()
    let r_remove = root.clone();
    let id_remove = elem_id.clone();
    elem_obj.set(
        "remove",
        JsValue::native("remove", move |_vm, _args| {
            let mut borrowed = r_remove.borrow_mut();
            if let Some(node_id) = borrowed.find_by_id(&id_remove).map(|node| node.node_id()) {
                let _ = borrowed.detach_by_id(node_id);
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
    let canvas_registry = canvas_commands.clone();
    let webgl_registry = webgl_commands.clone();
    let canvas_node_id = internal_node_id;
    elem_obj.set(
        "getContext",
        JsValue::native("getContext", move |_vm, args| {
            let ctx_name = args.first().map(|a| a.to_js_string()).unwrap_or_else(|| "2d".to_string());
            if ctx_name == "2d" {
                Ok(JsValue::Object(Rc::new(RefCell::new(create_canvas_2d_context(
                    canvas_node_id,
                    canvas_registry.clone(),
                )))))
            } else if ctx_name == "webgl" || ctx_name == "experimental-webgl" {
                Ok(JsValue::Object(Rc::new(RefCell::new(create_webgl_context(
                    canvas_node_id,
                    webgl_registry.clone(),
                )))))
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

fn create_canvas_2d_context(
    node_id: u64,
    registry: Rc<RefCell<HashMap<u64, Vec<Canvas2DCommand>>>>,
) -> JsObject {
    let mut ctx = JsObject::new();
    let fill_style = Rc::new(RefCell::new("#000000".to_string()));
    let stroke_style = Rc::new(RefCell::new("#000000".to_string()));
    let line_width = Rc::new(Cell::new(1.0f32));

    ctx.set("fillStyle", JsValue::String("#000000".to_string()));
    ctx.set("strokeStyle", JsValue::String("#000000".to_string()));
    ctx.set("lineWidth", JsValue::Number(1.0));
    ctx.set("isCanvas2D", JsValue::Boolean(true));

    let fs = fill_style.clone();
    ctx.set(
        "setFillStyle",
        JsValue::native("setFillStyle", move |_vm, args| {
            *fs.borrow_mut() = args.first().map(|v| v.to_js_string()).unwrap_or_else(|| "#000000".into());
            Ok(JsValue::Undefined)
        }),
    );

    let ss = stroke_style.clone();
    ctx.set(
        "setStrokeStyle",
        JsValue::native("setStrokeStyle", move |_vm, args| {
            *ss.borrow_mut() = args.first().map(|v| v.to_js_string()).unwrap_or_else(|| "#000000".into());
            Ok(JsValue::Undefined)
        }),
    );

    let lw = line_width.clone();
    ctx.set(
        "setLineWidth",
        JsValue::native("setLineWidth", move |_vm, args| {
            lw.set(args.first().map(|v| v.to_number() as f32).unwrap_or(1.0).max(1.0));
            Ok(JsValue::Undefined)
        }),
    );

    let reg_fill = registry.clone();
    let fill_style_ref = fill_style.clone();
    ctx.set(
        "fillRect",
        JsValue::native("fillRect", move |_vm, args| {
            let x = args.get(0).map(|v| v.to_number() as f32).unwrap_or(0.0);
            let y = args.get(1).map(|v| v.to_number() as f32).unwrap_or(0.0);
            let width = args.get(2).map(|v| v.to_number() as f32).unwrap_or(0.0);
            let height = args.get(3).map(|v| v.to_number() as f32).unwrap_or(0.0);
            reg_fill.borrow_mut().entry(node_id).or_default().push(
                Canvas2DCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color: fill_style_ref.borrow().clone(),
                },
            );
            Ok(JsValue::Undefined)
        }),
    );

    let reg_stroke = registry.clone();
    let stroke_style_ref = stroke_style.clone();
    let line_width_ref = line_width.clone();
    ctx.set(
        "strokeRect",
        JsValue::native("strokeRect", move |_vm, args| {
            let x = args.get(0).map(|v| v.to_number() as f32).unwrap_or(0.0);
            let y = args.get(1).map(|v| v.to_number() as f32).unwrap_or(0.0);
            let width = args.get(2).map(|v| v.to_number() as f32).unwrap_or(0.0);
            let height = args.get(3).map(|v| v.to_number() as f32).unwrap_or(0.0);
            reg_stroke.borrow_mut().entry(node_id).or_default().push(
                Canvas2DCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    color: stroke_style_ref.borrow().clone(),
                    line_width: line_width_ref.get(),
                },
            );
            Ok(JsValue::Undefined)
        }),
    );

    // clearRect remains conservative until the display-list backend supports
    // destination-out/transparent replacement semantics.
    ctx.set("clearRect", JsValue::native("clearRect", |_vm, _args| Ok(JsValue::Undefined)));

    let path = Rc::new(RefCell::new(CanvasPathState::default()));

    let path_begin = path.clone();
    ctx.set("beginPath", JsValue::native("beginPath", move |_vm, _args| {
        *path_begin.borrow_mut() = CanvasPathState::default();
        Ok(JsValue::Undefined)
    }));

    let path_move = path.clone();
    ctx.set("moveTo", JsValue::native("moveTo", move |_vm, args| {
        let x = args.get(0).map(|v| v.to_number() as f32).unwrap_or(0.0);
        let y = args.get(1).map(|v| v.to_number() as f32).unwrap_or(0.0);
        let mut state = path_move.borrow_mut();
        state.current = Some((x, y));
        state.first = Some((x, y));
        Ok(JsValue::Undefined)
    }));

    let path_line = path.clone();
    ctx.set("lineTo", JsValue::native("lineTo", move |_vm, args| {
        let x = args.get(0).map(|v| v.to_number() as f32).unwrap_or(0.0);
        let y = args.get(1).map(|v| v.to_number() as f32).unwrap_or(0.0);
        let mut state = path_line.borrow_mut();
        let (x1, y1) = state.current.unwrap_or((0.0, 0.0));
        if state.first.is_none() {
            state.first = Some((x1, y1));
        }
        state.primitives.push(CanvasPathPrimitive::Line { x1, y1, x2: x, y2: y });
        state.current = Some((x, y));
        Ok(JsValue::Undefined)
    }));

    let path_arc = path.clone();
    ctx.set("arc", JsValue::native("arc", move |_vm, args| {
        let cx = args.get(0).map(|v| v.to_number() as f32).unwrap_or(0.0);
        let cy = args.get(1).map(|v| v.to_number() as f32).unwrap_or(0.0);
        let radius = args.get(2).map(|v| v.to_number() as f32).unwrap_or(0.0).max(0.0);
        if radius > 0.0 {
            let mut state = path_arc.borrow_mut();
            state.primitives.push(CanvasPathPrimitive::Circle { cx, cy, radius });
            let end_x = cx + radius;
            let end_y = cy;
            if state.first.is_none() {
                state.first = Some((end_x, end_y));
            }
            state.current = Some((end_x, end_y));
        }
        Ok(JsValue::Undefined)
    }));

    let path_close = path.clone();
    ctx.set("closePath", JsValue::native("closePath", move |_vm, _args| {
        let mut state = path_close.borrow_mut();
        if let (Some((x1, y1)), Some((x2, y2))) = (state.current, state.first) {
            if (x1 - x2).abs() > f32::EPSILON || (y1 - y2).abs() > f32::EPSILON {
                state.primitives.push(CanvasPathPrimitive::Line { x1, y1, x2, y2 });
            }
            state.current = Some((x2, y2));
        }
        Ok(JsValue::Undefined)
    }));

    let path_fill = path.clone();
    let reg_path_fill = registry.clone();
    let fill_for_path = fill_style.clone();
    ctx.set("fill", JsValue::native("fill", move |_vm, _args| {
        let color = fill_for_path.borrow().clone();
        let primitives = path_fill.borrow().primitives.clone();
        let mut registry = reg_path_fill.borrow_mut();
        let commands = registry.entry(node_id).or_default();
        for primitive in primitives {
            if let CanvasPathPrimitive::Circle { cx, cy, radius } = primitive {
                commands.push(Canvas2DCommand::FillCircle { cx, cy, radius, color: color.clone() });
            }
        }
        Ok(JsValue::Undefined)
    }));

    let path_stroke = path.clone();
    let reg_path_stroke = registry.clone();
    let stroke_for_path = stroke_style.clone();
    let width_for_path = line_width.clone();
    ctx.set("stroke", JsValue::native("stroke", move |_vm, _args| {
        let color = stroke_for_path.borrow().clone();
        let width = width_for_path.get();
        let primitives = path_stroke.borrow().primitives.clone();
        let mut registry = reg_path_stroke.borrow_mut();
        let commands = registry.entry(node_id).or_default();
        for primitive in primitives {
            match primitive {
                CanvasPathPrimitive::Line { x1, y1, x2, y2 } => {
                    commands.push(Canvas2DCommand::DrawLine {
                        x1, y1, x2, y2, color: color.clone(), line_width: width,
                    });
                }
                CanvasPathPrimitive::Circle { cx, cy, radius } => {
                    commands.push(Canvas2DCommand::StrokeCircle {
                        cx, cy, radius, color: color.clone(), line_width: width,
                    });
                }
            }
        }
        Ok(JsValue::Undefined)
    }));

    ctx
}

fn create_webgl_context(
    node_id: u64,
    registry: Rc<RefCell<HashMap<u64, Vec<WebGlCommand>>>>,
) -> JsObject {
    let mut gl = JsObject::new();
    gl.set("isWebGL", JsValue::Boolean(true));
    gl.set("NO_ERROR", JsValue::Number(0.0));
    gl.set("COLOR_BUFFER_BIT", JsValue::Number(0x4000 as f64));
    gl.set("DEPTH_BUFFER_BIT", JsValue::Number(0x0100 as f64));
    gl.set("TRIANGLES", JsValue::Number(0x0004 as f64));
    gl.set("ARRAY_BUFFER", JsValue::Number(0x8892 as f64));
    gl.set("ELEMENT_ARRAY_BUFFER", JsValue::Number(0x8893 as f64));
    gl.set("STATIC_DRAW", JsValue::Number(0x88E4 as f64));
    gl.set("FLOAT", JsValue::Number(0x1406 as f64));
    gl.set("UNSIGNED_SHORT", JsValue::Number(0x1403 as f64));
    gl.set("UNSIGNED_INT", JsValue::Number(0x1405 as f64));
    gl.set("VERTEX_SHADER", JsValue::Number(0x8B31 as f64));
    gl.set("FRAGMENT_SHADER", JsValue::Number(0x8B30 as f64));
    gl.set("COMPILE_STATUS", JsValue::Number(0x8B81 as f64));
    gl.set("LINK_STATUS", JsValue::Number(0x8B82 as f64));

    let reg_viewport = registry.clone();
    gl.set("viewport", JsValue::native("viewport", move |_vm, args| {
        let x = args.get(0).map(|v| v.to_number() as i32).unwrap_or(0);
        let y = args.get(1).map(|v| v.to_number() as i32).unwrap_or(0);
        let width = args.get(2).map(|v| v.to_number() as i32).unwrap_or(0).max(0);
        let height = args.get(3).map(|v| v.to_number() as i32).unwrap_or(0).max(0);
        reg_viewport.borrow_mut().entry(node_id).or_default().push(
            WebGlCommand::Viewport { x, y, width, height }
        );
        Ok(JsValue::Undefined)
    }));

    let reg_clear_color = registry.clone();
    gl.set("clearColor", JsValue::native("clearColor", move |_vm, args| {
        let color = [
            args.get(0).map(|v| v.to_number() as f32).unwrap_or(0.0).clamp(0.0, 1.0),
            args.get(1).map(|v| v.to_number() as f32).unwrap_or(0.0).clamp(0.0, 1.0),
            args.get(2).map(|v| v.to_number() as f32).unwrap_or(0.0).clamp(0.0, 1.0),
            args.get(3).map(|v| v.to_number() as f32).unwrap_or(0.0).clamp(0.0, 1.0),
        ];
        reg_clear_color.borrow_mut().entry(node_id).or_default().push(
            WebGlCommand::ClearColor(color)
        );
        Ok(JsValue::Undefined)
    }));

    let reg_clear = registry.clone();
    gl.set("clear", JsValue::native("clear", move |_vm, args| {
        let mask = args.first().map(|v| v.to_number() as u32).unwrap_or(0);
        reg_clear.borrow_mut().entry(node_id).or_default().push(
            WebGlCommand::Clear { mask }
        );
        Ok(JsValue::Undefined)
    }));

    gl.set("createBuffer", JsValue::native("createBuffer", |_vm, _args| {
        let id = ELEMENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst) as u32;
        let mut buf = JsObject::new();
        buf.set("_webglBufferId", JsValue::Number(id as f64));
        Ok(JsValue::Object(Rc::new(RefCell::new(buf))))
    }));

    let bound_buffer = Rc::new(Cell::new(None::<u32>));
    let bound_element_buffer = Rc::new(Cell::new(None::<u32>));
    let bound_for_bind = bound_buffer.clone();
    let bound_element_for_bind = bound_element_buffer.clone();
    let reg_bind = registry.clone();
    gl.set("bindBuffer", JsValue::native("bindBuffer", move |_vm, args| {
        let target = args.first().map(|v| v.to_number() as u32).unwrap_or(0);
        let id = args.get(1).and_then(webgl_object_id);
        match target {
            0x8892 => {
                bound_for_bind.set(id);
                reg_bind.borrow_mut().entry(node_id).or_default().push(
                    WebGlCommand::BindArrayBuffer(id)
                );
            }
            0x8893 => {
                bound_element_for_bind.set(id);
                reg_bind.borrow_mut().entry(node_id).or_default().push(
                    WebGlCommand::BindElementArrayBuffer(id)
                );
            }
            _ => {}
        }
        Ok(JsValue::Undefined)
    }));

    let bound_for_data = bound_buffer.clone();
    let bound_element_for_data = bound_element_buffer.clone();
    let reg_data = registry.clone();
    gl.set("bufferData", JsValue::native("bufferData", move |_vm, args| {
        let target = args.first().map(|v| v.to_number() as u32).unwrap_or(0);
        match target {
            0x8892 => {
                let Some(id) = bound_for_data.get() else {
                    return Ok(JsValue::Undefined);
                };
                let data = args.get(1).map(js_number_vec).unwrap_or_default();
                reg_data.borrow_mut().entry(node_id).or_default().push(
                    WebGlCommand::UploadArrayBuffer { id, data }
                );
            }
            0x8893 => {
                let Some(id) = bound_element_for_data.get() else {
                    return Ok(JsValue::Undefined);
                };
                let data = args.get(1).map(js_u32_vec).unwrap_or_default();
                reg_data.borrow_mut().entry(node_id).or_default().push(
                    WebGlCommand::UploadElementArrayBuffer { id, data }
                );
            }
            _ => {}
        }
        Ok(JsValue::Undefined)
    }));

    gl.set("createShader", JsValue::native("createShader", |_vm, args| {
        let id = ELEMENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst) as u32;
        let shader_type = args.first().map(|v| v.to_number()).unwrap_or(0.0);
        let mut shader = JsObject::new();
        shader.set("_webglShaderId", JsValue::Number(id as f64));
        shader.set("_webglShaderType", JsValue::Number(shader_type));
        shader.set("_webglSource", JsValue::String(String::new()));
        shader.set("_webglCompiled", JsValue::Boolean(false));
        Ok(JsValue::Object(Rc::new(RefCell::new(shader))))
    }));

    gl.set("shaderSource", JsValue::native("shaderSource", |_vm, args| {
        if let Some(JsValue::Object(shader)) = args.first() {
            let source = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();
            shader.borrow_mut().set("_webglSource", JsValue::String(source));
        }
        Ok(JsValue::Undefined)
    }));

    gl.set("compileShader", JsValue::native("compileShader", |_vm, args| {
        if let Some(JsValue::Object(shader)) = args.first() {
            let source = shader.borrow().get("_webglSource").to_js_string();
            shader.borrow_mut().set("_webglCompiled", JsValue::Boolean(!source.trim().is_empty()));
        }
        Ok(JsValue::Undefined)
    }));

    gl.set("getShaderParameter", JsValue::native("getShaderParameter", |_vm, args| {
        if let Some(JsValue::Object(shader)) = args.first() {
            return Ok(shader.borrow().get("_webglCompiled"));
        }
        Ok(JsValue::Boolean(false))
    }));

    gl.set("getShaderInfoLog", JsValue::native("getShaderInfoLog", |_vm, args| {
        if let Some(JsValue::Object(shader)) = args.first() {
            if shader.borrow().get("_webglCompiled").is_truthy() {
                return Ok(JsValue::String(String::new()));
            }
        }
        Ok(JsValue::String("WaveCore: shader source is empty or unsupported".to_string()))
    }));

    gl.set("createProgram", JsValue::native("createProgram", |_vm, _args| {
        let id = ELEMENT_ID_COUNTER.fetch_add(1, Ordering::SeqCst) as u32;
        let mut program = JsObject::new();
        program.set("_webglProgramId", JsValue::Number(id as f64));
        program.set("_webglAttachedCount", JsValue::Number(0.0));
        program.set("_webglLinked", JsValue::Boolean(false));
        Ok(JsValue::Object(Rc::new(RefCell::new(program))))
    }));

    gl.set("attachShader", JsValue::native("attachShader", |_vm, args| {
        if let Some(JsValue::Object(program)) = args.first() {
            let count = program.borrow().get("_webglAttachedCount").to_number();
            program.borrow_mut().set("_webglAttachedCount", JsValue::Number(count + 1.0));
        }
        Ok(JsValue::Undefined)
    }));

    gl.set("linkProgram", JsValue::native("linkProgram", |_vm, args| {
        if let Some(JsValue::Object(program)) = args.first() {
            let linked = program.borrow().get("_webglAttachedCount").to_number() >= 2.0;
            program.borrow_mut().set("_webglLinked", JsValue::Boolean(linked));
        }
        Ok(JsValue::Undefined)
    }));

    gl.set("getProgramParameter", JsValue::native("getProgramParameter", |_vm, args| {
        if let Some(JsValue::Object(program)) = args.first() {
            return Ok(program.borrow().get("_webglLinked"));
        }
        Ok(JsValue::Boolean(false))
    }));

    let reg_program = registry.clone();
    gl.set("useProgram", JsValue::native("useProgram", move |_vm, args| {
        let id = args.first().and_then(|value| match value {
            JsValue::Object(object) => {
                let raw = object.borrow().get("_webglProgramId").to_number();
                raw.is_finite().then_some(raw as u32)
            }
            JsValue::Null => None,
            _ => None,
        });
        reg_program.borrow_mut().entry(node_id).or_default().push(
            WebGlCommand::UseProgram(id)
        );
        Ok(JsValue::Undefined)
    }));

    let reg_pointer = registry.clone();
    let bound_for_pointer = bound_buffer.clone();
    gl.set("vertexAttribPointer", JsValue::native("vertexAttribPointer", move |_vm, args| {
        let index = args.get(0).map(|v| v.to_number() as u32).unwrap_or(0);
        let size = args.get(1).map(|v| v.to_number() as u32).unwrap_or(2).clamp(1, 4);
        let stride_bytes = args.get(4).map(|v| v.to_number() as u32).unwrap_or(0);
        let offset_bytes = args.get(5).map(|v| v.to_number() as u32).unwrap_or(0);
        reg_pointer.borrow_mut().entry(node_id).or_default().push(
            WebGlCommand::VertexAttribPointer {
                index,
                size,
                stride_floats: stride_bytes / 4,
                offset_floats: offset_bytes / 4,
                buffer_id: bound_for_pointer.get(),
            }
        );
        Ok(JsValue::Undefined)
    }));

    let reg_attr4f = registry.clone();
    gl.set("vertexAttrib4f", JsValue::native("vertexAttrib4f", move |_vm, args| {
        let index = args.get(0).map(|v| v.to_number() as u32).unwrap_or(0);
        let value = [
            args.get(1).map(|v| v.to_number() as f32).unwrap_or(0.0),
            args.get(2).map(|v| v.to_number() as f32).unwrap_or(0.0),
            args.get(3).map(|v| v.to_number() as f32).unwrap_or(0.0),
            args.get(4).map(|v| v.to_number() as f32).unwrap_or(1.0),
        ];
        reg_attr4f.borrow_mut().entry(node_id).or_default().push(
            WebGlCommand::VertexAttrib4f { index, value }
        );
        Ok(JsValue::Undefined)
    }));

    let reg_enable = registry.clone();
    gl.set("enableVertexAttribArray", JsValue::native("enableVertexAttribArray", move |_vm, args| {
        let index = args.first().map(|v| v.to_number() as u32).unwrap_or(0);
        reg_enable.borrow_mut().entry(node_id).or_default().push(
            WebGlCommand::EnableVertexAttribArray(index)
        );
        Ok(JsValue::Undefined)
    }));

    let reg_draw = registry.clone();
    gl.set("drawArrays", JsValue::native("drawArrays", move |_vm, args| {
        let mode = args.get(0).map(|v| v.to_number() as u32).unwrap_or(0x0004);
        let first = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
        let count = args.get(2).map(|v| v.to_number() as u32).unwrap_or(0);
        reg_draw.borrow_mut().entry(node_id).or_default().push(
            WebGlCommand::DrawArrays { mode, first, count }
        );
        Ok(JsValue::Undefined)
    }));

    let reg_draw_elements = registry.clone();
    gl.set("drawElements", JsValue::native("drawElements", move |_vm, args| {
        let mode = args.get(0).map(|v| v.to_number() as u32).unwrap_or(0x0004);
        let count = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
        let element_type = args.get(2).map(|v| v.to_number() as u32).unwrap_or(0x1403);
        let offset_bytes = args.get(3).map(|v| v.to_number() as u32).unwrap_or(0);
        reg_draw_elements.borrow_mut().entry(node_id).or_default().push(
            WebGlCommand::DrawElements { mode, count, element_type, offset_bytes }
        );
        Ok(JsValue::Undefined)
    }));

    gl.set("getError", JsValue::native("getError", |_vm, _args| {
        Ok(JsValue::Number(0.0))
    }));

    gl
}

fn webgl_object_id(value: &JsValue) -> Option<u32> {
    match value {
        JsValue::Object(object) => {
            let id = object.borrow().get("_webglBufferId").to_number();
            id.is_finite().then_some(id as u32)
        }
        JsValue::Null => None,
        _ => None,
    }
}

fn js_u32_vec(value: &JsValue) -> Vec<u32> {
    match value {
        JsValue::Array(items) => items
            .borrow()
            .iter()
            .map(|item| item.to_number())
            .filter(|number| number.is_finite() && *number >= 0.0)
            .map(|number| number as u32)
            .collect(),
        _ => value
            .to_js_string()
            .split(',')
            .filter_map(|part| part.trim().parse::<u32>().ok())
            .collect(),
    }
}

fn js_number_vec(value: &JsValue) -> Vec<f32> {
    match value {
        JsValue::Array(items) => items
            .borrow()
            .iter()
            .map(|item| item.to_number() as f32)
            .filter(|number| number.is_finite())
            .collect(),
        _ => value
            .to_js_string()
            .split(',')
            .filter_map(|part| part.trim().parse::<f32>().ok())
            .collect(),
    }
}

