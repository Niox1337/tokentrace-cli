//! ratatui screens and navigation.
//!
//! A read-only viewer over the store: overview, sources and adapters, session
//! list, and session detail. Measured and estimated token totals are always
//! shown on separate lines and never merged. The screen builders and key
//! handler stay terminal-free so they can be unit-tested.

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use rusqlite::Connection;

use crate::{adapters, store};

/// Which top-level screen the viewer is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Overview,
    Sources,
    Sessions,
    Detail,
    Breakdown,
    Warnings,
}

/// Tab order, shared by the tab bar and the left/right tab keys. Detail is a
/// sub-screen of Sessions, so it is not a tab.
const TABS: [(Screen, &str); 5] = [
    (Screen::Overview, "1 Overview"),
    (Screen::Sources, "2 Sources"),
    (Screen::Sessions, "3 Sessions"),
    (Screen::Breakdown, "4 Breakdown"),
    (Screen::Warnings, "5 Warnings"),
];

/// The loaded, read-only view of the store plus cursor state. Loaded once at
/// launch; session detail is fetched lazily when a session is opened. The
/// viewer never mutates the store.
pub struct App {
    pub overview: store::Overview,
    pub sources: Vec<store::SourceRow>,
    pub adapters: Vec<adapters::AdapterInfo>,
    pub sessions: Vec<store::SessionSummary>,
    pub breakdown: store::Breakdown,
    pub warnings: Vec<store::WarningRow>,
    pub screen: Screen,
    /// Cursor into `sessions` for the list and the opened detail.
    pub selected: usize,
    pub detail: Option<store::SessionDetail>,
    pub should_quit: bool,
}

impl App {
    /// Build the view from a store connection.
    pub fn load(conn: &Connection) -> anyhow::Result<Self> {
        Ok(App {
            overview: store::overview(conn)?,
            sources: store::list_sources(conn)?,
            adapters: adapters::list(),
            sessions: store::session_summaries(conn)?,
            breakdown: store::breakdown(conn)?,
            warnings: store::warning_breakdown(conn)?,
            screen: Screen::Overview,
            selected: 0,
            detail: None,
            should_quit: false,
        })
    }
}

/// Open the store viewer, restoring the terminal on the way out even on error.
pub fn run(conn: &Connection) -> anyhow::Result<()> {
    let mut app = App::load(conn)?;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let result = event_loop(&mut terminal, &mut app, conn);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn event_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    conn: &Connection,
) -> anyhow::Result<()> {
    while !app.should_quit {
        terminal.draw(|f| render(f, app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                handle_key(app, key.code);
                sync_detail(app, conn)?;
            }
        }
    }
    Ok(())
}

/// Apply one keypress. Navigation only, so it stays terminal-free and testable;
/// the detail load that an Enter implies is done by [`sync_detail`].
fn handle_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('1') => app.screen = Screen::Overview,
        KeyCode::Char('2') => app.screen = Screen::Sources,
        KeyCode::Char('3') => app.screen = Screen::Sessions,
        KeyCode::Char('4') => app.screen = Screen::Breakdown,
        KeyCode::Char('5') => app.screen = Screen::Warnings,
        KeyCode::Left => app.screen = cycle_tab(app.screen, -1),
        KeyCode::Right => app.screen = cycle_tab(app.screen, 1),
        KeyCode::Up => {
            if app.screen == Screen::Sessions {
                app.selected = app.selected.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if app.screen == Screen::Sessions && !app.sessions.is_empty() {
                app.selected = (app.selected + 1).min(app.sessions.len() - 1);
            }
        }
        KeyCode::Enter => {
            if app.screen == Screen::Sessions && !app.sessions.is_empty() {
                app.screen = Screen::Detail;
            }
        }
        KeyCode::Esc => match app.screen {
            Screen::Detail => app.screen = Screen::Sessions,
            _ => app.should_quit = true,
        },
        _ => {}
    }
}

/// Step `delta` tabs from the current screen, wrapping at the ends. A non-tab
/// screen (Detail) is left unchanged so the arrows do not jump out of it.
fn cycle_tab(current: Screen, delta: isize) -> Screen {
    let Some(i) = TABS.iter().position(|(s, _)| *s == current) else {
        return current;
    };
    let n = TABS.len() as isize;
    TABS[(((i as isize + delta) % n + n) % n) as usize].0
}

/// Load the detail for the selected session when the detail screen is open and
/// the cached detail does not already match it.
fn sync_detail(app: &mut App, conn: &Connection) -> anyhow::Result<()> {
    if app.screen != Screen::Detail {
        return Ok(());
    }
    let want = app.sessions.get(app.selected).map(|s| s.id.clone());
    let have = app.detail.as_ref().map(|d| d.summary.id.clone());
    if want != have {
        app.detail = match want {
            Some(id) => store::session_detail(conn, &id)?,
            None => None,
        };
    }
    Ok(())
}

/// Render the current screen: a tab bar, the screen body, and a key footer.
pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    f.render_widget(Paragraph::new(tab_bar(app.screen)), chunks[0]);

    let (title, lines) = match app.screen {
        Screen::Overview => (" Overview ", overview_lines(app)),
        Screen::Sources => (" Sources & adapters ", sources_lines(app)),
        Screen::Sessions => (" Sessions ", sessions_lines(app)),
        Screen::Detail => (" Session detail ", detail_lines(app)),
        Screen::Breakdown => (" Breakdown ", breakdown_lines(app)),
        Screen::Warnings => (" Warnings ", warnings_lines(app)),
    };
    let body = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(body, chunks[1]);

    f.render_widget(Paragraph::new(Span::raw(footer(app.screen))), chunks[2]);
}

fn tab_bar(current: Screen) -> Line<'static> {
    let mut spans = vec![Span::raw("tokentrace  ")];
    for (screen, label) in TABS {
        let style = if screen == current {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!(" {label} "), style));
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

fn footer(screen: Screen) -> &'static str {
    match screen {
        Screen::Sessions => "1-5/arrows tabs  up/down select  enter open  q quit",
        Screen::Detail => "1-5 tabs  esc back  q quit",
        _ => "1-5/arrows tabs  q quit",
    }
}

fn overview_lines(app: &App) -> Vec<Line<'static>> {
    let ov = &app.overview;
    let mut lines = vec![
        kv("sources", ov.sources.to_string()),
        kv("sessions", ov.sessions.to_string()),
        kv("warnings", ov.warnings.to_string()),
        Line::raw(""),
        kv("measured tokens", ov.measured_tokens.to_string()),
        kv("estimated tokens", ov.estimated_tokens.to_string()),
        Line::raw(""),
    ];
    if ov.top_sessions.is_empty() {
        lines.push(Line::raw("No sessions yet. Import a source to begin."));
    } else {
        lines.push(Line::raw("Top sessions:"));
        for s in &ov.top_sessions {
            lines.push(Line::raw(format!("  {}", session_row(s))));
        }
    }
    lines
}

fn sources_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![Line::raw("Imported sources:")];
    if app.sources.is_empty() {
        lines.push(Line::raw("  none"));
    } else {
        for s in &app.sources {
            lines.push(Line::raw(format!(
                "  {}  {}  ({} {})",
                s.id, s.name, s.adapter_id, s.adapter_version
            )));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::raw("Bundled adapters:"));
    for a in &app.adapters {
        lines.push(Line::raw(format!("  {}  {}  [{}]", a.id, a.name, a.status)));
        lines.push(Line::raw(format!(
            "    {}",
            adapters::caps_summary(&a.capabilities)
        )));
    }
    lines
}

fn sessions_lines(app: &App) -> Vec<Line<'static>> {
    if app.sessions.is_empty() {
        return vec![Line::raw("No sessions yet. Import a source to begin.")];
    }
    app.sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let marker = if i == app.selected { "> " } else { "  " };
            let style = if i == app.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Line::styled(format!("{marker}{}", session_row(s)), style)
        })
        .collect()
}

fn detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(detail) = &app.detail else {
        return vec![Line::raw("No session selected.")];
    };
    let s = &detail.summary;
    let mut lines = vec![
        kv("session", s.id.clone()),
        kv("repo", s.repo.clone().unwrap_or_else(|| "-".to_string())),
        kv(
            "branch",
            s.branch.clone().unwrap_or_else(|| "-".to_string()),
        ),
        kv(
            "commits",
            format!(
                "{} -> {}",
                detail.commit_before.as_deref().unwrap_or("-"),
                detail.commit_after.as_deref().unwrap_or("-")
            ),
        ),
        kv("turns", detail.turns.to_string()),
        kv("measured tokens", s.measured_tokens.to_string()),
        kv("estimated tokens", s.estimated_tokens.to_string()),
        kv("cost", fmt_cost(s.cost_minor, &s.currency)),
        Line::raw(""),
        Line::raw(format!("Requests ({}):", detail.requests.len())),
    ];
    for r in &detail.requests {
        lines.push(Line::raw(format!(
            "  {} / {}  {} tokens [{}]{}",
            r.model,
            r.provider,
            r.tokens,
            r.confidence,
            ok_suffix(r.success),
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::raw(format!("Tools ({}):", detail.tools.len())));
    for t in &detail.tools {
        lines.push(Line::raw(format!(
            "  {}{}{}",
            t.name,
            ok_suffix(t.success),
            t.decision
                .as_deref()
                .map(|d| format!("  {d}"))
                .unwrap_or_default(),
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::raw(format!("Warnings ({}):", detail.warnings.len())));
    for (kind, message) in &detail.warnings {
        lines.push(Line::raw(format!("  [{kind}] {message}")));
    }
    lines
}

fn breakdown_lines(app: &App) -> Vec<Line<'static>> {
    let bd = &app.breakdown;
    let mut lines = vec![Line::raw("Tokens (measured and estimated kept apart):")];
    lines.push(token_parts_line("measured", &bd.tokens.measured));
    lines.push(token_parts_line("estimated", &bd.tokens.estimated));

    lines.push(Line::raw(""));
    lines.push(Line::raw(format!(
        "Cost by model ({}):",
        bd.cost_by_model.len()
    )));
    if bd.cost_by_model.is_empty() {
        lines.push(Line::raw("  none"));
    }
    for c in &bd.cost_by_model {
        lines.push(Line::raw(format!(
            "  {}  {}",
            c.model,
            fmt_cost(c.amount_minor, &c.currency)
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::raw(format!("Tools ({}):", bd.tools.len())));
    if bd.tools.is_empty() {
        lines.push(Line::raw("  none"));
    }
    for t in &bd.tools {
        lines.push(Line::raw(format!(
            "  {}  {} calls  {} failed",
            t.name, t.calls, t.failures
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::raw(format!("File impact ({}):", bd.files.len())));
    if bd.files.is_empty() {
        lines.push(Line::raw("  none"));
    }
    for f in &bd.files {
        lines.push(Line::raw(format!(
            "  {}  {} writes  +{} -{}",
            f.path, f.writes, f.lines_added, f.lines_removed
        )));
    }
    lines
}

fn warnings_lines(app: &App) -> Vec<Line<'static>> {
    if app.warnings.is_empty() {
        return vec![Line::raw("No warnings recorded.")];
    }
    let total: u64 = app.warnings.iter().map(|w| w.count).sum();
    let mut lines = vec![Line::raw(format!("Warnings ({total}):")), Line::raw("")];
    for w in &app.warnings {
        lines.push(Line::raw(format!(
            "  [{}] x{}  {}",
            w.kind, w.count, w.message
        )));
    }
    lines
}

fn token_parts_line(label: &str, p: &store::TokenParts) -> Line<'static> {
    Line::raw(format!(
        "  {label:>9}: total {}  (in {}  out {}  cache-read {}  cache-create {})",
        p.total, p.input, p.output, p.cache_read, p.cache_creation
    ))
}

fn session_row(s: &store::SessionSummary) -> String {
    let repo = s.repo.as_deref().unwrap_or("(no repo)");
    let branch = s.branch.as_deref().unwrap_or("-");
    format!(
        "{repo} @{branch}  measured {}  estimated {}  cost {}  [{}]",
        s.measured_tokens,
        s.estimated_tokens,
        fmt_cost(s.cost_minor, &s.currency),
        s.status,
    )
}

fn kv(key: &str, value: String) -> Line<'static> {
    Line::from(vec![Span::raw(format!("{key:>18}: ")), Span::raw(value)])
}

fn fmt_cost(minor: i64, currency: &Option<String>) -> String {
    match currency {
        Some(c) => format!("{}.{:02} {c}", minor / 100, (minor % 100).abs()),
        None => "-".to_string(),
    }
}

fn ok_suffix(success: Option<bool>) -> &'static str {
    match success {
        Some(true) => "  ok",
        Some(false) => "  failed",
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn plain(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn sample_session() -> store::SessionSummary {
        store::SessionSummary {
            id: "sess".to_string(),
            repo: Some("acme/widget".to_string()),
            branch: Some("main".to_string()),
            started_at: Some(10),
            status: "closed".to_string(),
            measured_tokens: 120,
            estimated_tokens: 40,
            cost_minor: 15,
            currency: Some("USD".to_string()),
        }
    }

    fn empty_app() -> App {
        App {
            overview: store::Overview::default(),
            sources: Vec::new(),
            adapters: adapters::list(),
            sessions: Vec::new(),
            breakdown: store::Breakdown::default(),
            warnings: Vec::new(),
            screen: Screen::Overview,
            selected: 0,
            detail: None,
            should_quit: false,
        }
    }

    fn populated_app() -> App {
        let s = sample_session();
        let detail = store::SessionDetail {
            summary: s.clone(),
            commit_before: Some("aaa".to_string()),
            commit_after: Some("bbb".to_string()),
            turns: 1,
            requests: vec![store::RequestRow {
                model: "claude-opus-4-8".to_string(),
                provider: "anthropic".to_string(),
                tokens: 120,
                confidence: "measured".to_string(),
                success: Some(true),
            }],
            tools: vec![store::ToolRow {
                name: "Edit".to_string(),
                success: Some(true),
                decision: Some("user_temporary".to_string()),
            }],
            warnings: vec![(
                "redaction".to_string(),
                "file attribution unavailable".to_string(),
            )],
        };
        App {
            overview: store::Overview {
                sources: 1,
                sessions: 1,
                warnings: 1,
                measured_tokens: 120,
                estimated_tokens: 40,
                top_sessions: vec![s.clone()],
            },
            sources: Vec::new(),
            adapters: adapters::list(),
            sessions: vec![s],
            breakdown: store::Breakdown {
                tokens: store::TokenBreakdown {
                    measured: store::TokenParts {
                        input: 100,
                        output: 20,
                        cache_read: 0,
                        cache_creation: 0,
                        total: 120,
                    },
                    estimated: store::TokenParts {
                        input: 30,
                        output: 10,
                        cache_read: 0,
                        cache_creation: 0,
                        total: 40,
                    },
                },
                cost_by_model: vec![store::CostByModel {
                    model: "claude-opus-4-8".to_string(),
                    amount_minor: 15,
                    currency: Some("USD".to_string()),
                }],
                tools: vec![store::ToolUsage {
                    name: "Edit".to_string(),
                    calls: 1,
                    failures: 0,
                }],
                files: Vec::new(),
            },
            warnings: vec![store::WarningRow {
                kind: "redaction".to_string(),
                message: "file attribution unavailable".to_string(),
                count: 1,
            }],
            screen: Screen::Detail,
            selected: 0,
            detail: Some(detail),
            should_quit: false,
        }
    }

    #[test]
    fn overview_keeps_measured_and_estimated_apart() {
        let mut app = populated_app();
        app.screen = Screen::Overview;
        let text = plain(&overview_lines(&app));
        assert!(text.contains("measured tokens: 120"));
        assert!(text.contains("estimated tokens: 40"));
        // The two totals are never combined into a single number.
        assert!(!text.contains("160"));
    }

    #[test]
    fn empty_overview_invites_an_import() {
        let text = plain(&overview_lines(&empty_app()));
        assert!(text.contains("sessions: 0"));
        assert!(text.contains("No sessions yet"));
    }

    #[test]
    fn detail_lists_requests_tools_and_warnings() {
        let text = plain(&detail_lines(&populated_app()));
        assert!(text.contains("claude-opus-4-8"));
        assert!(text.contains("Tools (1)"));
        assert!(text.contains("[redaction]"));
        assert!(text.contains("cost: 0.15 USD"));
    }

    #[test]
    fn breakdown_splits_token_bands_and_lists_models() {
        let mut app = populated_app();
        app.screen = Screen::Breakdown;
        let text = plain(&breakdown_lines(&app));
        assert!(text.contains("measured: total 120"));
        assert!(text.contains("estimated: total 40"));
        // Bands are never combined into one figure.
        assert!(!text.contains("160"));
        assert!(text.contains("claude-opus-4-8"));
        assert!(text.contains("Edit  1 calls"));
    }

    #[test]
    fn empty_breakdown_shows_no_data_without_panicking() {
        let text = plain(&breakdown_lines(&empty_app()));
        assert!(text.contains("measured: total 0"));
        assert!(text.contains("Cost by model (0)"));
    }

    #[test]
    fn key_four_opens_the_breakdown_screen() {
        let mut app = empty_app();
        handle_key(&mut app, KeyCode::Char('4'));
        assert_eq!(app.screen, Screen::Breakdown);
    }

    #[test]
    fn left_right_cycle_through_tabs_with_wrap() {
        let mut app = empty_app();
        // Right walks the tab order.
        handle_key(&mut app, KeyCode::Right);
        assert_eq!(app.screen, Screen::Sources);
        // Left from the first tab wraps to the last.
        app.screen = Screen::Overview;
        handle_key(&mut app, KeyCode::Left);
        assert_eq!(app.screen, Screen::Warnings);
        // Arrows leave the detail sub-screen unchanged.
        app.screen = Screen::Detail;
        handle_key(&mut app, KeyCode::Right);
        assert_eq!(app.screen, Screen::Detail);
    }

    #[test]
    fn warnings_screen_groups_with_counts_and_handles_empty() {
        let text = plain(&warnings_lines(&populated_app()));
        assert!(text.contains("[redaction] x1"));
        assert!(text.contains("file attribution unavailable"));

        let empty = plain(&warnings_lines(&empty_app()));
        assert!(empty.contains("No warnings recorded."));

        let mut app = empty_app();
        handle_key(&mut app, KeyCode::Char('5'));
        assert_eq!(app.screen, Screen::Warnings);
    }

    #[test]
    fn keys_navigate_screens_list_and_back() {
        let mut app = empty_app();
        app.sessions = vec![sample_session(), sample_session()];

        handle_key(&mut app, KeyCode::Char('3'));
        assert_eq!(app.screen, Screen::Sessions);
        // Down moves the cursor and clamps at the last row.
        handle_key(&mut app, KeyCode::Down);
        assert_eq!(app.selected, 1);
        handle_key(&mut app, KeyCode::Down);
        assert_eq!(app.selected, 1);
        // Enter opens the detail; Esc returns to the list, not quit.
        handle_key(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::Detail);
        handle_key(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Sessions);
        assert!(!app.should_quit);
        // Esc off the detail screen quits.
        handle_key(&mut app, KeyCode::Esc);
        assert!(app.should_quit);
    }

    #[test]
    fn enter_does_nothing_on_an_empty_session_list() {
        let mut app = empty_app();
        app.screen = Screen::Sessions;
        handle_key(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::Sessions);
    }

    #[test]
    fn every_screen_renders_on_empty_and_full_store() {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        for screen in [
            Screen::Overview,
            Screen::Sources,
            Screen::Sessions,
            Screen::Detail,
            Screen::Breakdown,
            Screen::Warnings,
        ] {
            let mut app = empty_app();
            app.screen = screen;
            terminal.draw(|f| render(f, &app)).unwrap();
            let mut full = populated_app();
            full.screen = screen;
            terminal.draw(|f| render(f, &full)).unwrap();
        }
    }
}
