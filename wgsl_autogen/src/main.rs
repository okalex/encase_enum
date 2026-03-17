use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "wgsl_autogen",
    about = "Generate WGSL type definitions from Rust source files"
)]
struct Cli {
    /// Input Rust source files or directories to scan
    #[arg(short, long, num_args = 1..)]
    input: Vec<PathBuf>,

    /// Output WGSL file path
    #[arg(short, long)]
    output: PathBuf,
}

fn main() {
    let cli = Cli::parse();

    let wgsl = match wgsl_autogen::generate_wgsl_from_files(&cli.input) {
        Ok(wgsl) => wgsl,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if let Some(parent) = cli.output.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    match std::fs::write(&cli.output, &wgsl) {
        Ok(_) => println!("Generated {} ({} bytes)", cli.output.display(), wgsl.len()),
        Err(e) => {
            eprintln!("Error writing {}: {}", cli.output.display(), e);
            std::process::exit(1);
        }
    }
}
