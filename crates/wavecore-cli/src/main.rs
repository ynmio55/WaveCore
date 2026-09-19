use std::cell::RefCell;
use std::rc::Rc;
use std::{env, fs, process};
use wavecore_dom::Node;
use wavecore_html::{extract_scripts, extract_styles};
use wavecore_js::{eval_script, DomBridge, JsObject, JsValue, VM};
use wavecore_layout::LayoutBox;
use wavecore_net::{fetch_resource, NavigationController};
use wavecore_pixels::{Rgba, Surface};
use wavecore_render::DisplayCommand;
use wavecore_storage::WebStorage;
use wavecore_window::{BrowserWindow, WindowEvent};

struct BrowserState {
    dom: Rc<RefCell<Node>>,
    vm: VM,
    bridge: DomBridge,
    focused_id: Option<String>,
}

fn prepare_document(
    url: &str,
    extra_css: &str,
) -> Result<(BrowserState, String), String> {
    let resp = fetch_resource(url).map_err(|e| format!("Failed to load '{url}': {e}"))?;
    let parsed_dom = wavecore_html::parse(&resp.content);
    let dom = Rc::new(RefCell::new(parsed_dom));

    let mut vm = VM::new();
    let bridge = DomBridge::new(dom.clone());
    bridge.attach_to_vm(&mut vm);

    // Setup LocalStorage in VM
    let local_storage = WebStorage::new_in_memory();
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
    };

    Ok((state, full_css))
}

fn layout_and_render(
    dom: &Node,
    css: &str,
    width: f32,
) -> (LayoutBox, Vec<DisplayCommand>) {
    let sheet = wavecore_css::parse(css);
    let styled = wavecore_style::style_tree(dom, &sheet);
    let layout = wavecore_layout::layout(&styled, width);
    let display_list = wavecore_render::build_display_list(&layout);
    (layout, display_list)
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
        println!("Controls:");
        println!("  - Left Click: Click links (<a href>) or form inputs/buttons");
        println!("  - Typing: Enter text into focused form input");
        println!("  - Backspace: Delete text in input, or navigate back");
        println!("  - Mouse Wheel / Up / Down: Scroll");
        println!("  - Alt+Left / Alt+Right: Navigate Back / Forward");
        println!("  - F5 / Ctrl+R: Reload");
        println!("  - ESC: Exit");

        let (mut state, mut full_css) = match prepare_document(&initial_url, &extra_css) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("wavecore error: {e}");
                process::exit(1);
            }
        };

        let (mut layout, mut display_list) = layout_and_render(&state.dom.borrow(), &full_css, width as f32);
        let mut surface = Surface::new(width as u32, height as u32);
        surface.clear(Rgba(13, 17, 23, 255));
        surface.paint(&display_list);
        let _ = win.present(&surface);

        while win.update() {
            let events = win.poll_events();
            let mut needs_re_render = false;
            let mut needs_navigate = None;

            for event in events {
                match event {
                    WindowEvent::Click { x, y } => {
                        // Check link click
                        if let Some(target_href) = layout.find_link_at(x, y) {
                            let resolved = nav.resolve_relative(&target_href);
                            println!("Navigating to: {resolved}");
                            needs_navigate = Some(resolved);
                        } else if let Some(control) = layout.find_form_control_at(x, y) {
                            if let Some(fid) = &control.form_id {
                                state.focused_id = Some(fid.clone());
                                println!("Focused form control: #{}", fid);

                                // If button or submit, dispatch click event to Pulse JS
                                if control.form_control_type.as_deref() == Some("button")
                                    || control.form_control_type.as_deref() == Some("submit")
                                {
                                    state.bridge.dispatch_event(&mut state.vm, fid, "click");
                                    needs_re_render = true;
                                }
                            }
                        } else {
                            state.focused_id = None;
                        }
                    }
                    WindowEvent::TextInput(ch) => {
                        if let Some(fid) = &state.focused_id {
                            let mut borrowed = state.dom.borrow_mut();
                            if let Some(node) = borrowed.find_by_id_mut(fid) {
                                if let wavecore_dom::NodeType::Element(e) = &mut node.node_type {
                                    let mut current = e.value().unwrap_or("").to_string();
                                    current.push(ch);
                                    e.set_attribute("value", current);
                                    needs_re_render = true;
                                }
                            }
                        }
                    }
                    WindowEvent::Backspace => {
                        if let Some(fid) = &state.focused_id {
                            let mut borrowed = state.dom.borrow_mut();
                            if let Some(node) = borrowed.find_by_id_mut(fid) {
                                if let wavecore_dom::NodeType::Element(e) = &mut node.node_type {
                                    let mut current = e.value().unwrap_or("").to_string();
                                    current.pop();
                                    e.set_attribute("value", current);
                                    needs_re_render = true;
                                }
                            }
                        } else if let Some(prev) = nav.go_back().map(str::to_string) {
                            println!("Navigating back to: {prev}");
                            needs_navigate = Some(prev);
                        }
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
                    _ => {}
                }
            }

            if let Some(target) = needs_navigate {
                nav.push(target.clone());
                win.set_title(&format!("WaveCore - {target}"));
                match prepare_document(&target, &extra_css) {
                    Ok((ns, nc)) => {
                        state = ns;
                        full_css = nc;
                        needs_re_render = true;
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
            }

            let scroll_y = win.scroll_y;

            if needs_re_render {
                let (nl, nd) = layout_and_render(&state.dom.borrow(), &full_css, width as f32);
                layout = nl;
                display_list = nd;
                surface.clear(Rgba(13, 17, 23, 255));
                surface.paint_offset(&display_list, 0.0, -scroll_y);

                if !surface.damage_rects.is_empty() {
                    let rects = surface.damage_rects.clone();
                    if let Err(e) = win.present_damage(&surface, &rects) {
                        eprintln!("wavecore: window damage render error: {e}");
                        break;
                    }
                    surface.damage_rects.clear();
                } else if let Err(e) = win.present(&surface) {
                    eprintln!("wavecore: window render error: {e}");
                    break;
                }
            } else {
                win.update();
            }
        }
    } else {
        let (state, full_css) = match prepare_document(&initial_url, &extra_css) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("wavecore error: {e}");
                process::exit(1);
            }
        };

        let (layout, display_list) = layout_and_render(&state.dom.borrow(), &full_css, 800.0);

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
