use std::{env, fs, process};
use wavecore_pixels::{Rgba, Surface};
use wavecore_window::BrowserWindow;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: wavecore <file.html> [file.css] [--gui] [-o <output.ppm>]");
        process::exit(2);
    }

    let mut html_path = None;
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
                if html_path.is_none() {
                    html_path = Some(args[i].clone());
                } else if css_path.is_none() {
                    css_path = Some(args[i].clone());
                }
            }
        }
        i += 1;
    }

    let Some(path) = html_path else {
        eprintln!("usage: wavecore <file.html> [file.css] [--gui] [-o <output.ppm>]");
        process::exit(2);
    };

    let source = fs::read_to_string(&path).unwrap_or_else(|err| {
        eprintln!("wavecore: cannot read {path}: {err}");
        process::exit(1);
    });

    let css_source = css_path
        .map(|p| {
            fs::read_to_string(&p).unwrap_or_else(|e| {
                eprintln!("wavecore: cannot read {p}: {e}");
                process::exit(1);
            })
        })
        .unwrap_or_default();

    let dom = wavecore_html::parse(&source);
    let sheet = wavecore_css::parse(&css_source);
    let styled = wavecore_style::style_tree(&dom, &sheet);

    if gui_mode {
        let mut width = 800;
        let mut height = 600;

        let mut win = BrowserWindow::new("WaveCore Browser", width, height).unwrap_or_else(|e| {
            eprintln!("wavecore: failed to open window: {e}");
            process::exit(1);
        });

        println!("WaveCore window opened (60 FPS). Use mouse wheel to scroll, ESC to exit.");

        let mut layout = wavecore_layout::layout(&styled, width as f32);
        let mut display_list = wavecore_render::build_display_list(&layout);
        let mut surface = Surface::new(width as u32, height as u32);

        while win.update() {
            if let Some((nw, nh)) = win.check_resize() {
                width = nw;
                height = nh;
                layout = wavecore_layout::layout(&styled, width as f32);
                display_list = wavecore_render::build_display_list(&layout);
                surface = Surface::new(width as u32, height as u32);
            }

            let scroll_y = win.handle_scroll();
            surface.clear(Rgba(255, 255, 255, 255));
            surface.paint_offset(&display_list, 0.0, -scroll_y);

            if let Err(e) = win.present(&surface) {
                eprintln!("wavecore: window render error: {e}");
                break;
            }
        }
    } else {
        let layout = wavecore_layout::layout(&styled, 800.0);
        let display_list = wavecore_render::build_display_list(&layout);

        let mut surface = Surface::new(800, 600);
        surface.paint(&display_list);

        if let Some(out) = output_path {
            fs::write(&out, surface.to_ppm()).unwrap_or_else(|e| {
                eprintln!("wavecore: cannot write {out}: {e}");
                process::exit(1);
            });
            println!("WaveCore rendered {} display commands to {out}", display_list.len());
        }
    }
}
