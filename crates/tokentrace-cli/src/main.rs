use clap::{Parser, Subcommand};

mod adapters;
mod git;
mod store;
mod tui;

#[derive(Parser)]
#[command(
    name = "tokentrace",
    version,
    about = "Local-first token and cost profiler for coding agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report environment and store status.
    Doctor,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor => doctor(),
    }
}

fn doctor() {
    println!("tokentrace {}", env!("CARGO_PKG_VERSION"));
    println!(
        "  os:   {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    let status = store::status(store::default_store_path());
    println!("  store: {}", status.path.display());
    if status.exists {
        let size = status.size_bytes.unwrap_or(0);
        println!("    state:  present ({size} bytes)");
    } else {
        println!("    state:  not created yet");
    }
    println!("    sqlite: {}", status.sqlite_version);
}
