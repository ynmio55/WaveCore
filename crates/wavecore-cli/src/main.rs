use std::{env, fs, process};

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: wavecore <file.html>");
        process::exit(2);
    };

    let source = fs::read_to_string(&path).unwrap_or_else(|err| {
        eprintln!("wavecore: cannot read {path}: {err}");
        process::exit(1);
    });

    let dom = wavecore_html::parse(&source);
    println!("{dom:#?}");
}
