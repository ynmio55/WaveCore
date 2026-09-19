use std::{env, fs, process};
use wavecore_html::extract_styles;
use wavecore_layout::LayoutBox;
use wavecore_net::{fetch_resource, NavigationController};
use wavecore_pixels::{Rgba, Surface};
use wavecore_render::DisplayCommand;
use wavecore_window::{BrowserWindow, WindowEvent};

fn render_document(
    url: &str,
    extra_css: &str,
    width: f32,
) -> Result<(LayoutBox, Vec<DisplayCommand>), String> {
    let resp = fetch_resource(url).map_err(|e| format!("Failed to load '{url}': {e}"))?;
    let dom = wavecore_html::parse(&resp.content);

    let embedded_css = extract_styles(&dom);
    let full_css = format!("{extra_css}\n{embedded_css}");
    let sheet = wavecore_css::parse(&full_css);

    let styled = wavecore_style::style_tree(&dom, &sheet);
    let layout = wavecore_layout::layout(&styled, width);
    let display_list = wavecore_render::build_display_list(&layout);

    Ok((layout, display_list))
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

        println!("WaveCore window opened (60 FPS).");
        println!("Controls:");
        println!("  - Left Click: Click links (<a href>)");
        println!("  - Mouse Wheel / Up / Down: Scroll");
        println!("  - Backspace / Alt+Left: Go Back");
        println!("  - Alt+Right: Go Forward");
        println!("  - F5 / Ctrl+R: Reload");
        println!("  - ESC: Exit");

        let (mut layout, mut display_list) = match render_document(&initial_url, &extra_css, width as f32) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("wavecore error: {e}");
                process::exit(1);
            }
        };

        let mut surface = Surface::new(width as u32, height as u32);

        while win.update() {
            let events = win.poll_events();
            let mut needs_reload = false;

            for event in events {
                match event {
                    WindowEvent::Click { x, y } => {
                        if let Some(target_href) = layout.find_link_at(x, y) {
                            let resolved = nav.resolve_relative(&target_href);
                            println!("Navigating to: {resolved}");
                            nav.push(resolved);
                            needs_reload = true;
                        }
                    }
                    WindowEvent::NavigateBack => {
                        if let Some(prev) = nav.go_back().map(str::to_string) {
                            println!("Navigating back to: {prev}");
                            needs_reload = true;
                        }
                    }
                    WindowEvent::NavigateForward => {
                        if let Some(next) = nav.go_forward().map(str::to_string) {
                            println!("Navigating forward to: {next}");
                            needs_reload = true;
                        }
                    }
                    WindowEvent::Reload => {
                        println!("Reloading page...");
                        needs_reload = true;
                    }
                    _ => {}
                }
            }

            if let Some((nw, nh)) = win.check_resize() {
                width = nw;
                height = nh;
                needs_reload = true;
            }

            if needs_reload {
                if let Some(current) = nav.current_url() {
                    win.set_title(&format!("WaveCore - {current}"));
                    match render_document(current, &extra_css, width as f32) {
                        Ok((nl, nd)) => {
                            layout = nl;
                            display_list = nd;
                            surface = Surface::new(width as u32, height as u32);
                        }
                        Err(e) => {
                            eprintln!("wavecore navigation error: {e}");
                        }
                    }
                }
            }

            let scroll_y = win.scroll_y;
            surface.clear(Rgba(255, 255, 255, 255));
            surface.paint_offset(&display_list, 0.0, -scroll_y);

            if let Err(e) = win.present(&surface) {
                eprintln!("wavecore: window render error: {e}");
                break;
            }
        }
    } else {
        let (layout, display_list) = match render_document(&initial_url, &extra_css, 800.0) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("wavecore error: {e}");
                process::exit(1);
            }
        };

        let mut surface = Surface::new(800, 600);
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
