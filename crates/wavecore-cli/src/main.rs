use std::{env, fs, process};

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: wavecore <file.html> [file.css]");
        process::exit(2);
    };

    let source = fs::read_to_string(&path).unwrap_or_else(|err| {
        eprintln!("wavecore: cannot read {path}: {err}");
        process::exit(1);
    });
    let css_source = env::args().nth(2)
        .map(|p| fs::read_to_string(&p).unwrap_or_else(|e| {
            eprintln!("wavecore: cannot read {p}: {e}");
            process::exit(1);
        }))
        .unwrap_or_default();

    let dom = wavecore_html::parse(&source);
    let sheet = wavecore_css::parse(&css_source);
    let styled = wavecore_style::style_tree(&dom, &sheet);
    let layout = wavecore_layout::layout(&styled, 800.0);
    let display_list = wavecore_render::build_display_list(&layout);

    println!("{display_list:#?}");
}
