use std::{io, path::PathBuf, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame, Terminal,
};
use rusqlite::Connection;
use woderrise_core::{catalog, BackupManager, Database, InstallationEngine, Manifest};

const TABS: [&str; 5] = ["Catalog", "Install", "Rollback", "History", "Journal"];

#[derive(Debug, Clone)]
struct CatalogItem {
    id: String,
    name: String,
    description: String,
    category: String,
    version: String,
    manifest_dir: PathBuf,
    manifest_path: PathBuf,
    manifest: Manifest,
    installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransactionItem {
    id: String,
    package: String,
    version: String,
    kind: String,
    status: String,
    started_at: i64,
    operations: usize,
    backups: usize,
}

#[derive(Debug)]
struct App {
    page: usize,
    catalog: Vec<CatalogItem>,
    transactions: Vec<TransactionItem>,
    catalog_selection: usize,
    transaction_selection: usize,
    search: String,
    searching: bool,
    category: Option<String>,
    detail: bool,
    help: bool,
    confirm_rollback: bool,
    confirm_install: bool,
    status: String,
    db_available: bool,
    journal_path: PathBuf,
    journal_error: Option<String>,
    catalog_error: Option<String>,
    interrupted_transactions: usize,
    data_dir: PathBuf,
}

impl App {
    fn new() -> Self {
        let data_dir = data_dir();
        let mut app = Self {
            page: 0,
            catalog: Vec::new(),
            transactions: Vec::new(),
            catalog_selection: 0,
            transaction_selection: 0,
            search: String::new(),
            searching: false,
            category: None,
            detail: false,
            help: false,
            confirm_rollback: false,
            confirm_install: false,
            status: "Offline (journal unavailable)".into(),
            db_available: false,
            journal_path: data_dir.join("journal.db"),
            journal_error: None,
            catalog_error: None,
            interrupted_transactions: 0,
            data_dir,
        };
        app.reload();
        app
    }

    fn reload(&mut self) {
        let path = self.data_dir.join("journal.db");
        self.journal_path = path.clone();
        self.journal_error = None;
        self.catalog_error = None;
        match Database::open(&path) {
            Ok(db) => {
                match validate_journal(&db.conn) {
                    Ok(()) => {
                        self.db_available = true;
                        self.status = "Journal online".into();
                    }
                    Err(error) => {
                        self.db_available = false;
                        self.journal_error = Some(error);
                        self.status = "Journal unavailable".into();
                    }
                }
                self.catalog = load_catalog(&db, &registry_dir(), &mut self.catalog_error);
                self.transactions = load_transactions(&db.conn);
                self.interrupted_transactions = count_interrupted(&db.conn);
            }
            Err(error) => {
                self.db_available = false;
                self.journal_error = Some(error.to_string());
                self.status = format!("Backend unavailable: {error}");
                self.catalog.clear();
                self.transactions.clear();
            }
        }
        self.clamp_selection();
    }

    fn filtered_catalog(&self) -> Vec<&CatalogItem> {
        let query = self.search.to_lowercase();
        self.catalog
            .iter()
            .filter(|item| {
                (query.is_empty()
                    || item.name.to_lowercase().contains(&query)
                    || item.id.to_lowercase().contains(&query)
                    || item.description.to_lowercase().contains(&query))
                    && self
                        .category
                        .as_ref()
                        .is_none_or(|category| category == &item.category)
            })
            .collect()
    }

    fn selected_catalog(&self) -> Option<&CatalogItem> {
        self.filtered_catalog().get(self.catalog_selection).copied()
    }

    fn selected_transaction(&self) -> Option<&TransactionItem> {
        self.visible_transactions(self.page == 2)
            .get(self.transaction_selection)
            .copied()
    }

    fn visible_transactions(&self, rollback: bool) -> Vec<&TransactionItem> {
        let query = self.search.to_lowercase();
        self.transactions
            .iter()
            .filter(|tx| {
                (!rollback || tx.status == "COMMITTED")
                    && (query.is_empty()
                        || tx.id.to_lowercase().contains(&query)
                        || tx.package.to_lowercase().contains(&query)
                        || tx.status.to_lowercase().contains(&query))
            })
            .collect()
    }

    fn clamp_selection(&mut self) {
        let catalog_len = self.filtered_catalog().len();
        if catalog_len == 0 {
            self.catalog_selection = 0;
        } else {
            self.catalog_selection = self.catalog_selection.min(catalog_len - 1);
        }
        let transaction_len = self.visible_transactions(self.page == 2).len();
        if transaction_len == 0 {
            self.transaction_selection = 0;
        } else {
            self.transaction_selection = self.transaction_selection.min(transaction_len - 1);
        }
    }

    fn move_selection(&mut self, delta: i32) {
        if self.page == 0 || self.page == 1 {
            let len = self.filtered_catalog().len();
            self.catalog_selection = move_index(self.catalog_selection, len, delta);
        } else {
            self.transaction_selection = move_index(
                self.transaction_selection,
                self.visible_transactions(self.page == 2).len(),
                delta,
            );
        }
    }

    fn next_category(&mut self) {
        let mut categories: Vec<_> = self
            .catalog
            .iter()
            .map(|item| item.category.clone())
            .collect();
        categories.sort();
        categories.dedup();
        if categories.is_empty() {
            self.status = "No catalog categories available".into();
            return;
        }
        self.category = match self.category.as_ref() {
            None => Some(categories[0].clone()),
            Some(current) => categories
                .iter()
                .position(|category| category == current)
                .and_then(|index| categories.get(index + 1).cloned()),
        };
        self.catalog_selection = 0;
        self.status = self.category.as_ref().map_or_else(
            || "Showing all categories".into(),
            |c| format!("Category: {c}"),
        );
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.help = false;
            }
            return false;
        }
        if self.confirm_rollback {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.confirm_rollback = false;
                    self.perform_rollback();
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.confirm_rollback = false;
                    self.status = "Rollback cancelled".into();
                }
                _ => {}
            }
            return false;
        }
        if self.confirm_install {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.confirm_install = false;
                    self.perform_install();
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.confirm_install = false;
                    self.status = "Installation cancelled".into();
                }
                _ => {}
            }
            return false;
        }
        if self.searching {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.searching = false,
                KeyCode::Backspace => {
                    self.search.pop();
                    self.catalog_selection = 0;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search.push(c);
                    self.catalog_selection = 0;
                }
                _ => {}
            }
            self.clamp_selection();
            return false;
        }
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Left => {
                self.page = self.page.saturating_sub(1);
                self.clamp_selection();
            }
            KeyCode::Right => {
                self.page = (self.page + 1) % TABS.len();
                self.clamp_selection();
            }
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Char('/') if self.page == 0 || self.page == 1 || self.page == 3 => {
                self.searching = true
            }
            KeyCode::Char('c') if self.page == 0 => self.next_category(),
            KeyCode::Enter if self.page == 0 => self.detail = true,
            KeyCode::Enter if self.page == 2 || self.page == 3 => self.detail = true,
            KeyCode::Char('i') if self.page == 0 => {
                if self.selected_catalog().is_some() {
                    self.page = 1;
                    self.status = "Install plan loaded; press i to confirm".into();
                } else {
                    self.status = "Select a catalog package first".into();
                }
            }
            KeyCode::Char('i') if self.page == 1 => {
                if !self.db_available {
                    self.status = "Installation blocked: journal unavailable".into();
                } else if let Some(item) = self.selected_catalog() {
                    if item.manifest.install.files.is_empty() {
                        self.status =
                            "Installation unavailable: manifest has no file operations".into();
                    } else {
                        self.confirm_install = true;
                    }
                } else {
                    self.status = "Select an installation candidate first".into();
                }
            }
            KeyCode::Char('r') if self.page == 2 => {
                if self.selected_transaction().is_some() {
                    self.confirm_rollback = true;
                } else {
                    self.status = "No transaction selected".into();
                }
            }
            KeyCode::Esc => {
                if self.detail {
                    self.detail = false;
                } else if self.page > 0 {
                    self.page -= 1;
                    self.clamp_selection();
                }
            }
            KeyCode::Char('R') => self.reload(),
            _ => {}
        }
        false
    }

    fn perform_rollback(&mut self) {
        let Some(transaction) = self.selected_transaction().cloned() else {
            return;
        };
        if transaction.status != "COMMITTED" {
            self.status = format!("Rollback blocked: transaction is {}", transaction.status);
            return;
        }
        if !self.db_available {
            self.status = "Rollback blocked: journal unavailable".into();
            return;
        }
        let result = (|| -> woderrise_core::Result<String> {
            let db = Database::open(self.data_dir.join("journal.db"))?;
            let engine = InstallationEngine {
                db,
                backups: BackupManager::new(self.data_dir.join("backups"))?,
                home: home_dir(),
            };
            engine.rollback(&transaction.id, false)
        })();
        self.status = match result {
            Ok(id) => format!("Rollback completed: {id}"),
            Err(error) => format!("Rollback failed: {error}"),
        };
        self.reload();
    }

    fn perform_install(&mut self) {
        let Some(item) = self.selected_catalog().cloned() else {
            self.status = "Select an installation candidate first".into();
            return;
        };
        if !self.db_available {
            self.status = "Installation blocked: journal unavailable".into();
            return;
        }
        let result = (|| -> woderrise_core::Result<String> {
            let db = Database::open(self.data_dir.join("journal.db"))?;
            let engine = InstallationEngine {
                db,
                backups: BackupManager::new(self.data_dir.join("backups"))?,
                home: home_dir(),
            };
            engine.install(&item.manifest_path, "user_tui")
        })();
        self.status = match result {
            Ok(id) => format!("Installation committed: {id}"),
            Err(error) => format!("Installation failed: {error}"),
        };
        self.reload();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run(&mut terminal);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if app.handle_key(key) {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn draw(frame: &mut Frame<'_>, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let tabs = Tabs::new(
        TABS.iter()
            .map(|tab| Line::from(Span::raw(*tab)))
            .collect::<Vec<_>>(),
    )
    .select(app.page)
    .highlight_style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Woderrise ")
            .title_bottom(Line::from(format!(" {} ", app.status)).right_aligned()),
    );
    frame.render_widget(tabs, outer[0]);
    match app.page {
        0 => draw_catalog(frame, app, outer[1]),
        1 => draw_install(frame, app, outer[1]),
        2 => draw_transactions(frame, app, outer[1], true),
        3 => draw_transactions(frame, app, outer[1], false),
        _ => draw_journal(frame, app, outer[1]),
    }
    let footer = if app.searching {
        format!("Search: {}_", app.search)
    } else if app.page == 0 {
        "↑↓ select • / search • c category • Enter details • i install • ? help • q quit".into()
    } else if app.page == 1 {
        "←→ pages • i confirm install • Esc back • R refresh • ? help • q quit".into()
    } else if app.page == 2 {
        "↑↓ select • r rollback • Enter details • ? help • q quit".into()
    } else if app.page == 3 {
        "↑↓ select • / filter • Enter details • ? help • q quit".into()
    } else {
        "R refresh • ? help • q quit".into()
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
        outer[2],
    );
    if app.help {
        draw_help(frame);
    } else if app.detail {
        draw_detail(frame, app);
    } else if app.confirm_rollback {
        draw_confirmation(
            frame,
            "Confirm rollback",
            "The rollback engine will verify current file hashes before restoring backups.",
        );
    } else if app.confirm_install {
        draw_confirmation(
            frame,
            "Confirm installation",
            "The installer will validate the manifest, create backups, and modify declared files.",
        );
    }
}

fn draw_catalog(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    let entries = app.filtered_catalog();
    let items = entries
        .iter()
        .map(|item| {
            ListItem::new(Line::from(vec![
                Span::styled(&item.name, Style::default().fg(Color::White)),
                Span::styled(
                    format!("  v{}  {}", item.version, item.category),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let title = if let Some(category) = &app.category {
        format!("Catalog • {category} • {}", entries.len())
    } else {
        format!("Catalog • {}", entries.len())
    };
    let mut state = ListState::default();
    state.select((!entries.is_empty()).then_some(app.catalog_selection));
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::default().bg(Color::Blue).fg(Color::White)),
        columns[0],
        &mut state,
    );
    let detail = if let Some(item) = app.selected_catalog() {
        vec![
            Line::styled(
                &item.name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(format!("{}  •  {}", item.id, item.version)),
            Line::from(format!(
                "Type: {}  •  Category: {}",
                item.manifest.package.package_type, item.category
            )),
            Line::from(""),
            Line::from(item.description.clone()),
            Line::from(""),
            Line::styled("Install status", Style::default().fg(Color::Yellow)),
            Line::from(if item.installed {
                "Installed (journal ownership record found)."
            } else {
                "Not installed (no ownership record found)."
            }),
            Line::from(""),
            Line::styled(
                "Compatibility & dependencies",
                Style::default().fg(Color::Yellow),
            ),
            Line::from(format!("Manifest: {}", item.manifest_dir.display())),
            Line::from(format!(
                "File operations: {}",
                item.manifest.install.files.len()
            )),
            Line::from(format!(
                "Dependencies: {}",
                item.manifest
                    .dependencies
                    .as_ref()
                    .map_or(0, |d| d.packages.len())
            )),
            Line::from(format!(
                "Repository: {}",
                item.manifest
                    .package
                    .repository
                    .as_deref()
                    .unwrap_or("not declared")
            )),
        ]
    } else {
        vec![
            Line::styled("Catalog is empty", Style::default().fg(Color::Yellow)),
            Line::from("No local catalog entries are available."),
            Line::from("Populate the catalog database or press R to refresh."),
        ]
    };
    frame.render_widget(
        Paragraph::new(Text::from(detail))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Package details"),
            ),
        columns[1],
    );
}

fn draw_install(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);
    let selected = app.selected_catalog();
    let left = if let Some(item) = selected {
        vec![
            Line::styled("Selected candidate", Style::default().fg(Color::Cyan)),
            Line::from(item.name.as_str()),
            Line::from(format!("Version {}", item.version)),
            Line::from(format!("Type/category: {}", item.category)),
        ]
    } else {
        vec![Line::styled(
            "No installation candidate selected",
            Style::default().fg(Color::Yellow),
        )]
    };
    frame.render_widget(
        Paragraph::new(left).block(Block::default().borders(Borders::ALL).title("Install plan")),
        columns[0],
    );
    let plan = selected
        .map(|item| {
            item.manifest
                .install
                .files
                .iter()
                .map(|file| format!("{} -> {}", file.source, file.target))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut right = vec![
        Line::styled("Installer status", Style::default().fg(Color::Yellow)),
        Line::from(if selected.is_some() {
            "Validated manifest candidate."
        } else {
            "No candidate selected."
        }),
        Line::from(""),
        Line::from("Planned file changes:"),
        Line::from(""),
        Line::styled(
            if plan.is_empty() {
                "No file operations; installation is disabled."
            } else {
                "Press i to confirm real installation."
            },
            Style::default().fg(Color::Green),
        ),
    ];
    right.extend(plan.into_iter().map(Line::from));
    frame.render_widget(
        Paragraph::new(right)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Transaction")),
        columns[1],
    );
}

fn draw_transactions(frame: &mut Frame<'_>, app: &App, area: Rect, rollback: bool) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(area);
    let visible = app.visible_transactions(rollback);
    let items = visible
        .iter()
        .map(|tx| {
            ListItem::new(Line::from(vec![
                Span::raw(&tx.package),
                Span::styled(
                    format!("  {}  {}", tx.kind, tx.status),
                    Style::default().fg(status_color(&tx.status)),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let title = if rollback {
        "Rollback candidates"
    } else {
        "Transaction history"
    };
    let mut state = ListState::default();
    state.select(
        (!visible.is_empty()).then_some(
            app.transaction_selection
                .min(visible.len().saturating_sub(1)),
        ),
    );
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("{title} • {}", visible.len())),
            )
            .highlight_style(Style::default().bg(Color::Blue)),
        columns[0],
        &mut state,
    );
    let detail = if let Some(tx) = app.selected_transaction() {
        vec![
            Line::styled(
                &tx.package,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(format!("Transaction: {}", tx.id)),
            Line::from(format!("Type: {}  Version: {}", tx.kind, tx.version)),
            Line::from(format!("Started: {}", format_time(tx.started_at))),
            Line::from(format!("Status: {}", tx.status)),
            Line::from(format!(
                "Operations: {}  Backups: {}",
                tx.operations, tx.backups
            )),
            Line::from(""),
            Line::from(if rollback && tx.status == "COMMITTED" {
                "Rollback is available. Press r, then y to confirm."
            } else if rollback {
                "Rollback is blocked unless status is COMMITTED."
            } else {
                "Enter opens transaction details."
            }),
        ]
    } else {
        vec![
            Line::styled(
                if rollback {
                    "No rollback candidates"
                } else {
                    "No transaction history"
                },
                Style::default().fg(Color::Yellow),
            ),
            Line::from("Completed installations and rollbacks will appear here."),
        ]
    };
    frame.render_widget(
        Paragraph::new(detail).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Transaction details"),
        ),
        columns[1],
    );
}

fn draw_journal(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(area);
    let health = if app.db_available {
        "HEALTHY"
    } else {
        "UNAVAILABLE"
    };
    let mut left = vec![
        Line::styled(
            "Journal health",
            Style::default().fg(if app.db_available {
                Color::Green
            } else {
                Color::Red
            }),
        ),
        Line::from(health),
        Line::from(format!("Location: {}", app.journal_path.display())),
        Line::from(format!("Transactions: {}", app.transactions.len())),
        Line::from(format!(
            "Interrupted/unresolved: {}",
            app.interrupted_transactions
        )),
    ];
    if let Some(error) = &app.journal_error {
        left.push(Line::from(""));
        left.push(Line::styled(
            format!("Error: {error}"),
            Style::default().fg(Color::Red),
        ));
    }
    frame.render_widget(
        Paragraph::new(left)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Journal")),
        columns[0],
    );
    let catalog_message = app
        .catalog_error
        .as_deref()
        .unwrap_or("Manifest catalog loaded without parse errors.");
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("Catalog source", Style::default().fg(Color::Cyan)),
            Line::from(registry_dir().display().to_string()),
            Line::from(format!("Entries loaded: {}", app.catalog.len())),
            Line::from(""),
            Line::from(catalog_message),
            Line::from(""),
            Line::from("R refreshes journal and catalog data."),
        ])
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Persistence status"),
        ),
        columns[1],
    );
}

fn draw_detail(frame: &mut Frame<'_>, app: &App) {
    let area = centered(frame.area(), 70, 60);
    frame.render_widget(Clear, area);
    let body = if app.page == 0 {
        app.selected_catalog()
            .map(|item| {
                vec![
                    Line::styled(
                        &item.name,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Line::from(format!("ID: {}", item.id)),
                    Line::from(format!("Version: {}", item.version)),
                    Line::from(format!("Category: {}", item.category)),
                    Line::from(""),
                    Line::from(item.description.clone()),
                    Line::from(""),
                    Line::from(format!("Manifest: {}", item.manifest_dir.display())),
                    Line::from(
                        "Configuration inspection is limited to manifest-declared file operations.",
                    ),
                ]
            })
            .unwrap_or_else(|| vec![Line::from("No package selected.")])
    } else {
        app.selected_transaction()
            .map(|tx| {
                vec![
                    Line::styled(&tx.id, Style::default().fg(Color::Cyan)),
                    Line::from(format!("{} {} {}", tx.package, tx.kind, tx.status)),
                    Line::from(format!(
                        "{} operations, {} backups",
                        tx.operations, tx.backups
                    )),
                    Line::from("Operation-level results are available in journal.db."),
                ]
            })
            .unwrap_or_else(|| vec![Line::from("No transaction selected.")])
    };
    frame.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Details • Esc closes"),
        ),
        area,
    );
}

fn draw_help(frame: &mut Frame<'_>) {
    let area = centered(frame.area(), 62, 70);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "Keyboard help",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(""),
            Line::from("Left/Right  Switch pages"),
            Line::from("Up/Down     Move selection"),
            Line::from("/           Search (Catalog/History)"),
            Line::from("c           Cycle catalog category"),
            Line::from("Enter       Open details"),
            Line::from("i           Select install candidate"),
            Line::from("r           Request rollback"),
            Line::from("y / n       Confirm / cancel rollback"),
            Line::from("i           Confirm installation on Install page"),
            Line::from("R           Refresh database data"),
            Line::from("Esc         Close overlay or cancel"),
            Line::from("?           Close this help"),
            Line::from("q           Quit"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Help")),
        area,
    );
}

fn draw_confirmation(frame: &mut Frame<'_>, title: &str, message: &str) {
    let area = centered(frame.area(), 60, 30);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                title,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(""),
            Line::from(message),
            Line::from("No simulated operation or success status is shown."),
            Line::from(""),
            Line::from("Press y to continue, n or Esc to cancel."),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Safety confirmation"),
        ),
        area,
    );
}

fn load_catalog(db: &Database, root: &PathBuf, error: &mut Option<String>) -> Vec<CatalogItem> {
    if let Err(e) = catalog::import_registry(db, root) {
        *error = Some(e.to_string());
    }
    let records = match catalog::search(db, "", 1000) {
        Ok(records) => records,
        Err(e) => {
            *error = Some(e.to_string());
            return Vec::new();
        }
    };
    let mut entries = Vec::new();
    for record in records {
        match catalog::manifest(db, &record.id, &record.version) {
            Ok(manifest) => {
                let installed = db.conn.query_row(
                    "SELECT 1 FROM installed_packages WHERE package_id=?1 AND active_version=?2 LIMIT 1",
                    rusqlite::params![manifest.package.id, manifest.package.version],
                    |_| Ok(true),
                ).unwrap_or(false);
                entries.push(CatalogItem {
                    id: record.id,
                    name: record.name,
                    description: record.description,
                    category: record.category,
                    version: record.version,
                    manifest_dir: record.manifest_path.parent().unwrap_or(root).to_path_buf(),
                    manifest_path: record.manifest_path,
                    manifest,
                    installed,
                });
            }
            Err(e) => *error = Some(e.to_string()),
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn validate_journal(conn: &Connection) -> Result<(), String> {
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if mode.to_lowercase() != "wal" {
        return Err(format!("journal mode is {mode}, expected WAL"));
    }
    conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|error| error.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM transactions", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn count_interrupted(conn: &Connection) -> usize {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE status IN ('PLANNED','PREPARED','EXECUTING','FAILED')",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0) as usize
}

fn load_transactions(conn: &Connection) -> Vec<TransactionItem> {
    let Ok(mut statement) = conn.prepare("SELECT txn_id,package_id,version,txn_type,status,started_at FROM transactions ORDER BY started_at DESC") else { return Vec::new() };
    let Ok(rows) = statement.query_map([], |row| {
        let id: String = row.get(0)?;
        let operations: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM operations WHERE txn_id=?1",
                [&id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize;
        let backups: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM backup_snapshots WHERE txn_id=?1",
                [&id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize;
        Ok(TransactionItem {
            id,
            package: row.get(1)?,
            version: row.get(2)?,
            kind: row.get(3)?,
            status: row.get(4)?,
            started_at: row.get(5)?,
            operations,
            backups,
        })
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn move_index(index: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    if delta < 0 {
        index.saturating_sub(1)
    } else {
        (index + 1).min(len - 1)
    }
}

fn status_color(status: &str) -> Color {
    match status {
        "COMMITTED" | "ROLLED_BACK" => Color::Green,
        "FAILED" | "ABORTED_RECOVERED" => Color::Red,
        _ => Color::Yellow,
    }
}

fn format_time(timestamp: i64) -> String {
    if timestamp <= 0 {
        return "unknown".into();
    }
    let seconds = timestamp / 1000;
    format!("{seconds}s since Unix epoch")
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn data_dir() -> PathBuf {
    std::env::var_os("WODERRISE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local/share/woderrise"))
}

fn registry_dir() -> PathBuf {
    std::env::var_os("WODERRISE_REGISTRY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("registry/packages")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_loads_valid_registry_packages_without_hardcoded_entries() {
        let db = Database::in_memory().expect("database");
        let mut error = None;
        let entries = load_catalog(
            &db,
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages"),
            &mut error,
        );
        assert_eq!(entries.len(), 1);
        assert!(error.is_none());
        assert_eq!(entries[0].manifest.package.id, "com.catppuccin.waybar");
    }

    #[test]
    fn rollback_candidates_exclude_failed_transactions() {
        let mut app = App {
            page: 2,
            catalog: Vec::new(),
            transactions: Vec::new(),
            catalog_selection: 0,
            transaction_selection: 0,
            search: String::new(),
            searching: false,
            category: None,
            detail: false,
            help: false,
            confirm_rollback: false,
            confirm_install: false,
            status: String::new(),
            db_available: false,
            journal_path: PathBuf::new(),
            journal_error: None,
            catalog_error: None,
            interrupted_transactions: 0,
            data_dir: PathBuf::new(),
        };
        app.transactions = vec![
            TransactionItem {
                id: "committed".into(),
                package: "pkg".into(),
                version: "1".into(),
                kind: "INSTALL".into(),
                status: "COMMITTED".into(),
                started_at: 1,
                operations: 1,
                backups: 1,
            },
            TransactionItem {
                id: "failed".into(),
                package: "pkg".into(),
                version: "1".into(),
                kind: "INSTALL".into(),
                status: "FAILED".into(),
                started_at: 2,
                operations: 0,
                backups: 0,
            },
        ];
        app.page = 2;
        assert_eq!(app.visible_transactions(true).len(), 1);
        assert_eq!(
            app.selected_transaction().map(|tx| tx.id.as_str()),
            Some("committed")
        );
    }

    #[test]
    fn selection_movement_is_bounded_for_empty_and_nonempty_lists() {
        assert_eq!(move_index(0, 0, 1), 0);
        assert_eq!(move_index(0, 2, 1), 1);
        assert_eq!(move_index(1, 2, 1), 1);
        assert_eq!(move_index(1, 2, -1), 0);
    }
}
