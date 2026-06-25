use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use tokentrace_core::{AgentSource, PrivacyLevel, SourceType};

mod adapters;
mod git;
mod provider;
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
    /// Import a source file through an adapter into the store.
    Import {
        /// Adapter id, e.g. claude-code.
        #[arg(long)]
        adapter: String,
        /// Path to the source export to import.
        #[arg(long)]
        path: PathBuf,
        /// Human-readable name for the source (defaults to the file name).
        #[arg(long)]
        name: Option<String>,
        /// Opt in to importing a source whose adapter can expose sensitive content.
        #[arg(long)]
        allow_sensitive: bool,
    },
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
    /// Discover and import local Claude Code and Codex session logs.
    Scan,
    /// Browse the store in the terminal UI.
    Tui,
    /// Export the store as newline-delimited JSON, one object per session.
    Export {
        /// File to write to; writes to stdout when omitted.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Summarize the current git repo and attribute a cost across a commit range.
    Git {
        /// Start revision of the range (exclusive), e.g. a session's commit-before.
        #[arg(long)]
        from: Option<String>,
        /// End revision of the range.
        #[arg(long, default_value = "HEAD")]
        to: String,
        /// Session cost in minor currency units (e.g. cents) to attribute across the range.
        #[arg(long)]
        cost: Option<i64>,
        /// Currency label for the attributed cost.
        #[arg(long, default_value = "USD")]
        currency: String,
    },
}

#[derive(Subcommand)]
enum SourcesCommand {
    /// List imported sources.
    List,
    /// Register a local source file for an adapter.
    Add {
        /// Adapter id, e.g. claude-code.
        #[arg(long)]
        adapter: String,
        /// Human-readable name for the source.
        #[arg(long)]
        name: String,
        /// Path to the local source file.
        #[arg(long)]
        path: PathBuf,
    },
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
        Command::Import {
            adapter,
            path,
            name,
            allow_sensitive,
        } => import(adapter, path, name, allow_sensitive)?,
        Command::Sources {
            command: SourcesCommand::List,
        } => sources_list()?,
        Command::Sources {
            command:
                SourcesCommand::Add {
                    adapter,
                    name,
                    path,
                },
        } => sources_add(adapter, name, path)?,
        Command::Adapters {
            command: AdaptersCommand::List,
        } => adapters_list(),
        Command::Scan => scan()?,
        Command::Tui => {
            let conn = store::open(&store::default_store_path())?;
            tui::run(&conn)?;
        }
        Command::Export { out } => export(out)?,
        Command::Git {
            from,
            to,
            cost,
            currency,
        } => git_summary(from, to, cost, currency)?,
    }
    Ok(())
}

/// Report the current repo, and when given a range and cost, attribute that cost
/// per commit, file, and line, falling back to a session-level total when commit
/// timing is ambiguous.
fn git_summary(
    from: Option<String>,
    to: String,
    cost: Option<i64>,
    currency: String,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let provider = git::GitProvider::discover(&cwd)?;
    println!("repo:   {}", provider.root().display());
    match provider.branch()? {
        Some(b) => println!("branch: {b}"),
        None => println!("branch: (detached HEAD)"),
    }
    println!("head:   {}", provider.head()?);

    let Some(from) = from else {
        return Ok(());
    };
    let stat = provider.diff_stat(&from, &to)?;
    let times = provider.commit_times(&from, &to)?;
    let lines = stat.additions + stat.deletions;
    println!(
        "range:  {from}..{to}  ({} commits, {} files, +{} -{})",
        times.len(),
        stat.files,
        stat.additions,
        stat.deletions
    );

    let Some(amount) = cost else {
        return Ok(());
    };
    // Take the range's own commit span as the session window; the timing gate in
    // `attribute` then rejects empty ranges and zero-change diffs as ambiguous.
    let window = (
        times.iter().copied().min().unwrap_or(0),
        times.iter().copied().max().unwrap_or(0),
    );
    let usage = tokentrace_core::CostUsage {
        amount_minor: amount,
        currency,
        pricing_source: "user".to_string(),
        confidence: tokentrace_core::Confidence::Estimated,
    };
    let commit_times: Vec<Option<i64>> = times.into_iter().map(Some).collect();
    match git::attribute(window, &commit_times, stat.files, lines, &usage) {
        git::Attribution::PerCommit(a) => {
            println!(
                "cost:   {} {}/commit, {} {}/file, {} {}/line [{:?}]",
                a.per_commit_minor,
                a.currency,
                a.per_file_minor,
                a.currency,
                a.per_line_minor,
                a.currency,
                a.confidence,
            );
        }
        git::Attribution::SessionOnly(w) => {
            println!(
                "cost:   {} {} for the range (not attributed)",
                usage.amount_minor, usage.currency
            );
            println!("        warning: {}", w.message);
        }
    }
    Ok(())
}

/// Discover local Claude Code and Codex session logs through each adapter's
/// `detect`, then import what they find. Safe to re-run; the import is
/// idempotent. The raw bytes are not stored, since native logs carry prompts.
fn scan() -> anyhow::Result<()> {
    let mut conn = store::open(&store::default_store_path())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64);

    let mut total_files = 0usize;
    let mut total_measured = 0u64;
    for info in adapters::list() {
        let Some(runner) = adapters::build(info.id) else {
            continue;
        };
        let detections = runner.detect()?;
        if detections.is_empty() {
            continue;
        }
        println!(
            "{} ({}): {} session file(s)",
            info.name,
            info.id,
            detections.len()
        );
        let mut measured = 0u64;
        for d in &detections {
            let path = PathBuf::from(&d.locator);
            let raw = match std::fs::read(&path) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("  skipped {}: {e}", path.display());
                    continue;
                }
            };
            let data = match runner.parse(&raw) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("  skipped {}: {e}", path.display());
                    continue;
                }
            };
            let warnings = runner.validate(&data);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| info.id.to_string());
            let source = AgentSource {
                id: source_id(info.id, &name, &path),
                name,
                source_type: SourceType::LocalFile,
                adapter_id: info.id.to_string(),
                adapter_version: info.version.to_string(),
                capabilities: info.capabilities,
                imported_at: now,
            };
            store::ensure_source(&conn, &source)?;
            // Empty raw keeps prompt-bearing native logs out of the store.
            let counts = store::import_parsed(&mut conn, &source.id, b"", &data, &warnings)?;
            measured += counts.measured_tokens;
            total_files += 1;
        }
        println!("  imported {measured} new measured tokens");
        total_measured += measured;
    }

    if total_files == 0 {
        println!("No local Claude Code or Codex session logs found.");
    } else {
        println!("Scanned {total_files} file(s); {total_measured} new measured tokens.");
    }
    Ok(())
}

fn export(out: Option<PathBuf>) -> anyhow::Result<()> {
    let conn = store::open(&store::default_store_path())?;
    match out {
        Some(path) => {
            let mut f = std::fs::File::create(&path)?;
            let n = store::export_jsonl(&conn, &mut f)?;
            println!("Exported {n} sessions to {}", path.display());
        }
        None => {
            store::export_jsonl(&conn, &mut std::io::stdout().lock())?;
        }
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

fn sources_add(adapter: String, name: String, path: PathBuf) -> anyhow::Result<()> {
    let info = adapters::find(&adapter).ok_or_else(|| {
        anyhow::anyhow!("unknown adapter '{adapter}'; see `tokentrace adapters list`")
    })?;
    if !path.exists() {
        anyhow::bail!("path does not exist: {}", path.display());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64);
    let source = AgentSource {
        id: source_id(&adapter, &name, &path),
        name,
        source_type: SourceType::LocalFile,
        adapter_id: adapter,
        adapter_version: info.version.to_string(),
        capabilities: info.capabilities,
        imported_at: now,
    };

    let conn = store::open(&store::default_store_path())?;
    store::insert_source(&conn, &source)?;

    println!("Added source '{}' ({})", source.name, source.id);
    println!(
        "  adapter:      {} {}",
        source.adapter_id, source.adapter_version
    );
    println!(
        "  capabilities: {}",
        adapters::caps_summary(&source.capabilities)
    );
    Ok(())
}

fn import(
    adapter: String,
    path: PathBuf,
    name: Option<String>,
    allow_sensitive: bool,
) -> anyhow::Result<()> {
    let info = adapters::find(&adapter).ok_or_else(|| {
        anyhow::anyhow!("unknown adapter '{adapter}'; see `tokentrace adapters list`")
    })?;
    // Importing sensitive content stays opt-in and is labelled when it happens.
    let sensitive = info.capabilities.privacy_level == PrivacyLevel::Sensitive;
    if sensitive && !allow_sensitive {
        anyhow::bail!(
            "adapter '{adapter}' can expose sensitive content; re-run with --allow-sensitive to opt in"
        );
    }
    let runner = adapters::build(&adapter)
        .ok_or_else(|| anyhow::anyhow!("adapter '{adapter}' cannot import yet"))?;

    let raw =
        std::fs::read(&path).map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    let data = runner.parse(&raw)?;
    let warnings = runner.validate(&data);

    let name = name.unwrap_or_else(|| {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| adapter.clone())
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64);
    let source = AgentSource {
        id: source_id(&adapter, &name, &path),
        name,
        source_type: SourceType::LocalFile,
        adapter_id: adapter,
        adapter_version: info.version.to_string(),
        capabilities: info.capabilities,
        imported_at: now,
    };

    let mut conn = store::open(&store::default_store_path())?;
    store::ensure_source(&conn, &source)?;
    let counts = store::import_parsed(&mut conn, &source.id, &raw, &data, &warnings)?;

    let label = if sensitive { "  [sensitive]" } else { "" };
    println!("Imported '{}' ({}){}", source.name, source.id, label);
    println!(
        "  sessions: {}  turns: {}  requests: {}  tools: {}",
        counts.sessions, counts.turns, counts.requests, counts.tools
    );
    println!("  measured tokens: {}", counts.measured_tokens);
    if counts.warnings > 0 {
        println!("  warnings: {}", counts.warnings);
        for w in &warnings {
            println!("    [{:?}] {}", w.kind, w.message);
        }
    }
    Ok(())
}

/// A short, stable id for a local source, derived from its adapter, name, and path.
fn source_id(adapter: &str, name: &str, path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(adapter.as_bytes());
    h.update([0]);
    h.update(name.as_bytes());
    h.update([0]);
    h.update(path.to_string_lossy().as_bytes());
    h.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn adapters_list() {
    for a in adapters::list() {
        println!("{}  {}  [{}]", a.id, a.name, a.status);
        println!("    {}", adapters::caps_summary(&a.capabilities));
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
