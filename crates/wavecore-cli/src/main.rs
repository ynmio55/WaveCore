use std::cell::RefCell;
use std::rc::Rc;
use std::{env, fs, path::PathBuf, process};
use wavecore_dom::{Node, NodeId, NodeType};
use wavecore_html::{extract_scripts, extract_styles};
use wavecore_js::{eval_script, DomBridge, JsObject, JsValue, VM};
use wavecore_layout::{LayoutBox, Rect};
use wavecore_net::{NavigationController, NetworkClient};
use wavecore_pixels::{Rgba, Surface};
use wavecore_render::{CompositorFrame, DisplayCommand};
use wavecore_sandbox::Origin;
use wavecore_storage::WebStorage;
use wavecore_window::{BrowserWindow, WindowEvent};

struct BrowserState {
    dom: Rc<RefCell<Node>>,
    vm: VM,
    bridge: DomBridge,
    focused_id: Option<String>,
    focused_node_id: Option<NodeId>,
    composition: String,
}

fn profile_root() -> PathBuf {
    if let Ok(custom) = env::var("WAVECORE_PROFILE_DIR") {
        return PathBuf::from(custom);
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".local").join("share").join("wavecore");
    }
    if let Ok(profile) = env::var("USERPROFILE") {
        return PathBuf::from(profile)
            .join("AppData")
            .join("Local")
            .join("WaveCore");
    }
    PathBuf::from(".wavecore")
}

fn prepare_document(
    url: &str,
    extra_css: &str,
    network_client: Rc<RefCell<NetworkClient>>,
) -> Result<(BrowserState, String), String> {
    let resp = network_client
        .borrow_mut()
        .fetch(url)
        .map_err(|e| format!("Failed to load '{url}': {e}"))?;
    let parsed_dom = wavecore_html::parse(&resp.content);
    let dom = Rc::new(RefCell::new(parsed_dom));

    let mut vm = VM::new();
    let bridge = DomBridge::with_url_and_client(dom.clone(), url, network_client);
    bridge.attach_to_vm(&mut vm);

    // Persistent localStorage, isolated by the current page/origin key.
    let storage_dir = profile_root().join("local-storage");
    let storage_origin = Origin::parse(url)
        .map(|o| o.to_string_repr())
        .unwrap_or_else(|_| url.to_string());
    let local_storage = WebStorage::new_persistent(&storage_dir, &storage_origin);
    let ls_ref = Rc::new(RefCell::new(local_storage));
    let mut ls_obj = JsObject::new();

    let ls_get = ls_ref.clone();
    ls_obj.set(
        "getItem",
        JsValue::native("getItem", move |_vm, args| {
            let key = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let val = ls_get.borrow().get_item(&key).map(|s| s.to_string());
            match val {
                Some(v) => Ok(JsValue::String(v)),
                None => Ok(JsValue::Null),
            }
        }),
    );

    let ls_set = ls_ref.clone();
    ls_obj.set(
        "setItem",
        JsValue::native("setItem", move |_vm, args| {
            let key = args.first().map(|a| a.to_js_string()).unwrap_or_default();
            let val = args.get(1).map(|a| a.to_js_string()).unwrap_or_default();
            ls_set.borrow_mut().set_item(key, val);
            Ok(JsValue::Undefined)
        }),
    );

    vm.set_global("localStorage", JsValue::Object(Rc::new(RefCell::new(ls_obj))));

    // Run embedded scripts
    let scripts = extract_scripts(&dom.borrow());
    for s in scripts {
        if let Err(e) = eval_script(&s, &mut vm) {
            eprintln!("[WaveCore Pulse JS Error] {e}");
        }
    }

    let embedded_css = extract_styles(&dom.borrow());
    let full_css = format!("{extra_css}\n{embedded_css}");

    let state = BrowserState {
        dom,
        vm,
        bridge,
        focused_id: None,
        focused_node_id: None,
        composition: String::new(),
    };

    Ok((state, full_css))
}

fn layout_and_render(
    dom: &Node,
    css: &str,
    width: f32,
    canvas_commands: &std::collections::HashMap<u64, Vec<wavecore_render::Canvas2DCommand>>,
) -> (LayoutBox, CompositorFrame, Vec<DisplayCommand>) {
    let sheet = wavecore_css::parse(css);
    let styled = wavecore_style::style_tree(dom, &sheet);
    let layout = wavecore_layout::layout(&styled, width);
    let mut compositor_frame = wavecore_render::build_compositor_frame(&layout);
    wavecore_render::append_canvas_to_compositor_frame(
        &layout,
        canvas_commands,
        &mut compositor_frame,
    );
    let display_list = compositor_frame.flatten();
    (layout, compositor_frame, display_list)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: wavecore <url_or_file> [file.css] [--gui] [-o <output.ppm>]");
        process::exit(2);
    }

    let mut target_url = None;
    let mut css_path = None;
    let mut output_path = Some("wavecore.ppm".to_string());
    let mut gui_mode = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--gui" | "-g" => {
                gui_mode = true;
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output_path = Some(args[i].clone());
                }
            }
            arg if arg.starts_with("--") => {
                eprintln!("wavecore: unknown option '{arg}'");
            }
            _ => {
                if target_url.is_none() {
                    target_url = Some(args[i].clone());
                } else if css_path.is_none() {
                    css_path = Some(args[i].clone());
                }
            }
        }
        i += 1;
    }

    let Some(initial_url) = target_url else {
        eprintln!("usage: wavecore <url_or_file> [file.css] [--gui] [-o <output.ppm>]");
        process::exit(2);
    };

    let extra_css = css_path
        .map(|p| {
            fs::read_to_string(&p).unwrap_or_else(|e| {
                eprintln!("wavecore: cannot read CSS {p}: {e}");
                process::exit(1);
            })
        })
        .unwrap_or_default();

    let mut nav = NavigationController::new(initial_url.clone());
    let network_client = Rc::new(RefCell::new(NetworkClient::new()));

    if gui_mode {
        let mut width = 800;
        let mut height = 600;

        let mut win = BrowserWindow::new(
            &format!("WaveCore - {}", initial_url),
            width,
            height,
        )
        .unwrap_or_else(|e| {
            eprintln!("wavecore: failed to open window: {e}");
            process::exit(1);
        });

        println!("WaveCore Browser Window opened (60 FPS).");
        if let Some(adapter) = win.gpu_adapter_name() {
            println!("GPU compositor: enabled via {adapter}");
        } else {
            println!("GPU compositor: unavailable; using software fallback");
        }
        println!("Controls:");
        println!("  - Left Click: Click links (<a href>) or form inputs/buttons");
        println!("  - Typing: Enter text into focused form input");
        println!("  - Backspace: Delete text in input, or navigate back");
        println!("  - Mouse Wheel / Up / Down: Scroll");
        println!("  - Alt+Left / Alt+Right: Navigate Back / Forward");
        println!("  - F5 / Ctrl+R: Reload");
        println!("  - ESC: Exit");

        let (mut state, mut full_css) = match prepare_document(&initial_url, &extra_css, network_client.clone()) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("wavecore error: {e}");
                process::exit(1);
            }
        };

        let initial_canvas = state.bridge.canvas_commands_snapshot();
        let (mut layout, mut compositor_frame, mut display_list) =
            layout_and_render(&state.dom.borrow(), &full_css, width as f32, &initial_canvas);
        let mut surface = Surface::new(width as u32, height as u32);
        let gpu_presented = win.present_compositor(&compositor_frame, 0.0).unwrap_or(false);
        if !gpu_presented {
            surface.clear(Rgba(13, 17, 23, 255));
            surface.paint(&display_list);
            let _ = win.present(&surface);
        }

        let mut focused_rect: Option<Rect> = None;

        while win.update() {
            let events = win.poll_events();
            let mut needs_re_render = false;
            let mut needs_repaint = false;
            let mut full_repaint = false;
            let mut damage_doc: Vec<Rect> = Vec::new();
            let mut needs_navigate = None;

            // Run due setTimeout callbacks on the browser event loop.
            // Timer callbacks may mutate the DOM, so conservatively relayout/repaint.
            if state.bridge.dispatch_due_timers(&mut state.vm) > 0 {
                needs_re_render = true;
                full_repaint = true;
            }

            for event in events {
                match event {
                    WindowEvent::Click { x, y } => {
                        // Check link click
                        if let Some(target_href) = layout.find_link_at(x, y) {
                            let resolved = nav.resolve_relative(&target_href);
                            println!("Navigating to: {resolved}");
                            needs_navigate = Some(resolved);
                        } else if let Some(control) = layout.find_form_control_at(x, y) {
                            if let Some(node_id) = control.node_id {
                                let fid = {
                                    let mut dom = state.dom.borrow_mut();
                                    dom.ensure_element_id(node_id, "_wc_focus_")
                                };
                                if let Some(fid) = fid {
                                    state.focused_id = Some(fid.clone());
                                    state.focused_node_id = Some(node_id);
                                    state.composition.clear();
                                    focused_rect = Some(control.rect);
                                    println!("Focused form control: #{}", fid);

                                    let control_type = control.form_control_type.as_deref().unwrap_or("");
                                    if matches!(control_type, "checkbox" | "radio") {
                                        let mut dom = state.dom.borrow_mut();
                                        if let Some(node) = dom.find_by_node_id_mut(node_id) {
                                            if let NodeType::Element(element) = &mut node.node_type {
                                                if control_type == "checkbox" {
                                                    element.set_checked(!element.is_checked());
                                                } else {
                                                    element.set_checked(true);
                                                }
                                            }
                                        }
                                        state.bridge.dispatch_event(&mut state.vm, &fid, "input");
                                        state.bridge.dispatch_event(&mut state.vm, &fid, "change");
                                        needs_re_render = true;
                                        full_repaint = true;
                                    } else if matches!(control_type, "button" | "submit") {
                                        let prevented =
                                            state.bridge.dispatch_event(&mut state.vm, &fid, "click");
                                        if !prevented {
                                            state.bridge.dispatch_event(&mut state.vm, &fid, "submit");
                                        }
                                        needs_re_render = true;
                                        full_repaint = true;
                                    } else {
                                        state.bridge.dispatch_event(&mut state.vm, &fid, "focus");
                                    }
                                }
                            }
                        } else {
                            if let Some(fid) = state.focused_id.take() {
                                state.bridge.dispatch_event(&mut state.vm, &fid, "blur");
                            }
                            state.focused_node_id = None;
                            state.composition.clear();
                            focused_rect = None;
                        }
                    }
                    WindowEvent::TextInput(text) => {
                        if let (Some(fid), Some(node_id)) =
                            (state.focused_id.clone(), state.focused_node_id)
                        {
                            let mut changed = false;
                            {
                                let mut borrowed = state.dom.borrow_mut();
                                if let Some(node) = borrowed.find_by_node_id_mut(node_id) {
                                    if let NodeType::Element(e) = &mut node.node_type {
                                        let control_type = e.input_type().to_ascii_lowercase();
                                        if !matches!(control_type.as_str(), "checkbox" | "radio" | "button" | "submit") {
                                            let mut current = e.value().unwrap_or("").to_string();
                                            current.push_str(&text);
                                            e.set_attribute("value", current);
                                            changed = true;
                                        }
                                    }
                                }
                            }
                            if changed {
                                state.bridge.dispatch_event_with_data(
                                    &mut state.vm,
                                    &fid,
                                    "input",
                                    Some(&text),
                                );
                                needs_re_render = true;
                                if let Some(rect) = focused_rect {
                                    damage_doc.push(rect);
                                } else {
                                    full_repaint = true;
                                }
                            }
                        }
                    }
                    WindowEvent::CompositionStart => {
                        state.composition.clear();
                        if let Some(fid) = state.focused_id.clone() {
                            state.bridge.dispatch_event(&mut state.vm, &fid, "compositionstart");
                        }
                    }
                    WindowEvent::CompositionUpdate(text) => {
                        state.composition = text.clone();
                        if let Some(fid) = state.focused_id.clone() {
                            state.bridge.dispatch_event_with_data(
                                &mut state.vm,
                                &fid,
                                "compositionupdate",
                                Some(&text),
                            );
                        }
                    }
                    WindowEvent::CompositionEnd(text) => {
                        state.composition.clear();
                        if let (Some(fid), Some(node_id)) =
                            (state.focused_id.clone(), state.focused_node_id)
                        {
                            {
                                let mut borrowed = state.dom.borrow_mut();
                                if let Some(node) = borrowed.find_by_node_id_mut(node_id) {
                                    if let NodeType::Element(e) = &mut node.node_type {
                                        let mut current = e.value().unwrap_or("").to_string();
                                        current.push_str(&text);
                                        e.set_attribute("value", current);
                                    }
                                }
                            }
                            state.bridge.dispatch_event_with_data(
                                &mut state.vm,
                                &fid,
                                "compositionend",
                                Some(&text),
                            );
                            state.bridge.dispatch_event_with_data(
                                &mut state.vm,
                                &fid,
                                "input",
                                Some(&text),
                            );
                            needs_re_render = true;
                            full_repaint = true;
                        }
                    }
                    WindowEvent::Backspace => {
                        if let (Some(fid), Some(node_id)) =
                            (state.focused_id.clone(), state.focused_node_id)
                        {
                            let mut changed = false;
                            {
                                let mut borrowed = state.dom.borrow_mut();
                                if let Some(node) = borrowed.find_by_node_id_mut(node_id) {
                                    if let NodeType::Element(e) = &mut node.node_type {
                                        let mut current = e.value().unwrap_or("").to_string();
                                        changed = current.pop().is_some();
                                        e.set_attribute("value", current);
                                    }
                                }
                            }
                            if changed {
                                state.bridge.dispatch_event(&mut state.vm, &fid, "input");
                                needs_re_render = true;
                                if let Some(rect) = focused_rect {
                                    damage_doc.push(rect);
                                } else {
                                    full_repaint = true;
                                }
                            }
                        } else if let Some(prev) = nav.go_back().map(str::to_string) {
                            println!("Navigating back to: {prev}");
                            needs_navigate = Some(prev);
                        }
                    }
                    WindowEvent::Enter => {
                        if let Some(fid) = state.focused_id.clone() {
                            state.bridge.dispatch_event(&mut state.vm, &fid, "change");
                            let prevented =
                                state.bridge.dispatch_event(&mut state.vm, &fid, "submit");
                            if !prevented {
                                if let Some(node_id) = state.focused_node_id {
                                    let payload = {
                                        let dom = state.dom.borrow();
                                        dom.parent_of(node_id)
                                            .filter(|parent| parent.tag_name() == Some("form"))
                                            .map(|form| form.form_urlencoded())
                                    };
                                    if let Some(payload) = payload {
                                        println!("[Form] submit: {}", payload);
                                    }
                                }
                            }
                        }
                    }
                    WindowEvent::Tab => {
                        if let Some(fid) = state.focused_id.take() {
                            state.bridge.dispatch_event(&mut state.vm, &fid, "change");
                            state.bridge.dispatch_event(&mut state.vm, &fid, "blur");
                        }
                        state.focused_node_id = None;
                        state.composition.clear();
                        focused_rect = None;
                    }
                    WindowEvent::NavigateBack => {
                        if let Some(prev) = nav.go_back().map(str::to_string) {
                            println!("Navigating back to: {prev}");
                            needs_navigate = Some(prev);
                        }
                    }
                    WindowEvent::NavigateForward => {
                        if let Some(next) = nav.go_forward().map(str::to_string) {
                            println!("Navigating forward to: {next}");
                            needs_navigate = Some(next);
                        }
                    }
                    WindowEvent::Reload => {
                        println!("Reloading page...");
                        if let Some(current) = nav.current_url() {
                            needs_navigate = Some(current.to_string());
                        }
                    }
                    WindowEvent::Scroll(_) => {
                        needs_repaint = true;
                        full_repaint = true;
                    }
                    _ => {}
                }
            }

            if let Some(target) = needs_navigate {
                nav.push(target.clone());
                win.set_title(&format!("WaveCore - {target}"));
                match prepare_document(&target, &extra_css, network_client.clone()) {
                    Ok((ns, nc)) => {
                        state = ns;
                        full_css = nc;
                        focused_rect = None;
                        state.focused_id = None;
                        state.focused_node_id = None;
                        state.composition.clear();
                        needs_re_render = true;
                        full_repaint = true;
                    }
                    Err(e) => {
                        eprintln!("Navigation load error: {e}");
                    }
                }
            }

            if let Some((nw, nh)) = win.check_resize() {
                width = nw;
                height = nh;
                surface = Surface::new(width as u32, height as u32);
                needs_re_render = true;
                full_repaint = true;
            }

            let scroll_y = win.scroll_y;

            if needs_re_render {
                let canvas_commands = state.bridge.canvas_commands_snapshot();
                let (nl, nf, nd) =
                    layout_and_render(&state.dom.borrow(), &full_css, width as f32, &canvas_commands);
                layout = nl;
                compositor_frame = nf;
                display_list = nd;
                needs_repaint = true;
            }

            if needs_repaint {
                if win.gpu_enabled() {
                    match win.present_compositor(&compositor_frame, scroll_y) {
                        Ok(true) => {
                            continue;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            eprintln!("wavecore: GPU compositor failed, falling back to software: {error}");
                            full_repaint = true;
                        }
                    }
                }

                if full_repaint || damage_doc.is_empty() {
                    // Damage is expressed in document coordinates so scrolling can be
                    // applied consistently by paint_damage_offset.
                    damage_doc.clear();
                    damage_doc.push(Rect {
                        x: 0.0,
                        y: scroll_y,
                        width: width as f32,
                        height: height as f32,
                    });
                }

                // Clear only damaged screen regions instead of repainting the full surface.
                let screen_damage: Vec<Rect> = damage_doc
                    .iter()
                    .map(|d| Rect {
                        x: d.x,
                        y: d.y - scroll_y,
                        width: d.width,
                        height: d.height,
                    })
                    .filter(|d| {
                        d.x + d.width > 0.0
                            && d.y + d.height > 0.0
                            && d.x < width as f32
                            && d.y < height as f32
                    })
                    .collect();

                for d in &screen_damage {
                    surface.fill_rect(d.x, d.y, d.width, d.height, Rgba(13, 17, 23, 255));
                }
                surface.paint_damage_offset(&display_list, &damage_doc, 0.0, -scroll_y);

                if let Err(e) = win.present_damage(&surface, &screen_damage) {
                    eprintln!("wavecore: window damage render error: {e}");
                    break;
                }
            }
        }
    } else {
        let (state, full_css) = match prepare_document(&initial_url, &extra_css, network_client.clone()) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("wavecore error: {e}");
                process::exit(1);
            }
        };

        let canvas_commands = state.bridge.canvas_commands_snapshot();
        let (layout, _compositor_frame, display_list) =
            layout_and_render(&state.dom.borrow(), &full_css, 800.0, &canvas_commands);

        let render_height = (layout.rect.height as u32 + 100).max(600).min(4000);
        let mut surface = Surface::new(800, render_height);
        surface.clear(Rgba(13, 17, 23, 255));
        surface.paint(&display_list);

        if let Some(out) = output_path {
            fs::write(&out, surface.to_ppm()).unwrap_or_else(|e| {
                eprintln!("wavecore: cannot write {out}: {e}");
                process::exit(1);
            });
            println!(
                "WaveCore rendered {} elements ({} display commands) to {out}",
                layout.children.len(),
                display_list.len()
            );
        }
    }
}
