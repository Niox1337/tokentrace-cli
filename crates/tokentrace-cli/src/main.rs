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
    /// Inspect imported sources.
    Sources {
        #[command(subcommand)]
        command: SourcesCommand,
    },
    /// Inspect bundled adapters.
    Adapters {
        #[command(subcommand)]
        command: AdaptersCommand,
    },
}

#[derive(Subcommand)]
enum SourcesCommand {
    /// List imported sources.
    List,
}

#[derive(Subcommand)]
enum AdaptersCommand {
    /// List bundled adapters.
    List,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor => doctor(),
        Command::Sources {
            command: SourcesCommand::List,
        } => sources_list()?,
        Command::Adapters {
            command: AdaptersCommand::List,
        } => adapters_list(),
    }
    Ok(())
}

fn sources_list() -> anyhow::Result<()> {
    let conn = store::open(&store::default_store_path())?;
    let sources = store::list_sources(&conn)?;
    if sources.is_empty() {
        println!("No sources imported yet.");
        return Ok(());
    }
    for s in sources {
        println!(
            "{}  {}  {}  ({} {})",
            s.id, s.name, s.source_type, s.adapter_id, s.adapter_version
        );
    }
    Ok(())
}

fn adapters_list() {
    for a in adapters::list() {
        println!("{}  {}  [{}]", a.id, a.name, a.status);
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
