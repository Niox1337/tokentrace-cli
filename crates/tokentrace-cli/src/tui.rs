//! ratatui screens and navigation.
//!
//! A read-only viewer over the store. This slice covers the overview and the
//! sources/adapters screens plus the event loop; the session list and detail
//! screens follow. Measured and estimated token totals are always shown on
//! separate lines and never merged. The screen builders stay pure so they can
//! be unit-tested without a terminal.

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
}

/// The loaded, read-only view of the store plus cursor state. Loaded once at
/// launch; the viewer does not mutate the store.
pub struct App {
    pub overview: store::Overview,
    pub sources: Vec<store::SourceRow>,
    pub adapters: Vec<adapters::AdapterInfo>,
    pub screen: Screen,
    pub should_quit: bool,
}

impl App {
    /// Build the view from a store connection.
    pub fn load(conn: &Connection) -> anyhow::Result<Self> {
        Ok(App {
            overview: store::overview(conn)?,
            sources: store::list_sources(conn)?,
            adapters: adapters::list(),
            screen: Screen::Overview,
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

    let result = event_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> anyhow::Result<()> {
    while !app.should_quit {
        terminal.draw(|f| render(f, app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                handle_key(app, key.code);
            }
        }
    }
    Ok(())
}

/// Apply one keypress. Kept terminal-free so navigation is unit-testable.
fn handle_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('1') => app.screen = Screen::Overview,
        KeyCode::Char('2') => app.screen = Screen::Sources,
        _ => {}
    }
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
    };
    let body = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(body, chunks[1]);

    f.render_widget(Paragraph::new(Span::raw("1/2 screens  q quit")), chunks[2]);
}

fn tab_bar(current: Screen) -> Line<'static> {
    let tabs = [
        (Screen::Overview, "1 Overview"),
        (Screen::Sources, "2 Sources"),
    ];
    let mut spans = vec![Span::raw("tokentrace  ")];
    for (screen, label) in tabs {
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

    fn empty_app() -> App {
        App {
            overview: store::Overview::default(),
            sources: Vec::new(),
            adapters: adapters::list(),
            screen: Screen::Overview,
            should_quit: false,
        }
    }

    fn populated_app() -> App {
        let s = store::SessionSummary {
            id: "sess".to_string(),
            repo: Some("acme/widget".to_string()),
            branch: Some("main".to_string()),
            started_at: Some(10),
            status: "closed".to_string(),
            measured_tokens: 120,
            estimated_tokens: 40,
            cost_minor: 15,
            currency: Some("USD".to_string()),
        };
        App {
            overview: store::Overview {
                sources: 1,
                sessions: 1,
                warnings: 1,
                measured_tokens: 120,
                estimated_tokens: 40,
                top_sessions: vec![s],
            },
            sources: Vec::new(),
            adapters: adapters::list(),
            screen: Screen::Overview,
            should_quit: false,
        }
    }

    #[test]
    fn overview_keeps_measured_and_estimated_apart() {
        let text = plain(&overview_lines(&populated_app()));
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
    fn keys_switch_screens_and_quit() {
        let mut app = empty_app();
        handle_key(&mut app, KeyCode::Char('2'));
        assert_eq!(app.screen, Screen::Sources);
        handle_key(&mut app, KeyCode::Char('1'));
        assert_eq!(app.screen, Screen::Overview);
        assert!(!app.should_quit);
        handle_key(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn both_screens_render_on_empty_and_full_store() {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        for screen in [Screen::Overview, Screen::Sources] {
            let mut app = empty_app();
            app.screen = screen;
            terminal.draw(|f| render(f, &app)).unwrap();
            let mut full = populated_app();
            full.screen = screen;
            terminal.draw(|f| render(f, &full)).unwrap();
        }
    }
}
