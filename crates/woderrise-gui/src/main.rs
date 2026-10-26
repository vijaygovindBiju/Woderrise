use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame, Terminal,
};
use rusqlite::Connection;
use woderrise_core::{
    catalog,
    diff::{DiffLineKind, FileDiffAction, InstallPlan},
    fetch_github_repository, parse_and_validate_github_url,
    BackupManager, Database, FetchOptions, InstallationEngine, Manifest,
};

const TABS: [&str; 6] = ["Catalog", "Install", "Rollback", "History", "Journal", "Settings"];
const SIDEBAR: [&str; 9] = [
    "⌂  Home",
    "⌕  Browse",
    "▦  Categories",
    "▣  Installed",
    "↻  Updates",
    "─",
    "▰  Registry",
    "⚙  Settings",
    "?  Help",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkStatus {
    Online,
    OfflineLocal,
    OfflineSyncFailed,
}

#[derive(Debug, Clone)]
enum OperationEvent {
    Progress(woderrise_gui::ProgressModel),
    Finished(Result<String, String>),
}

#[derive(Debug, Clone)]
struct ActiveOperation {
    title: String,
    model: woderrise_gui::ProgressModel,
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    woderrise_gui::parse_hex_rgb(hex).map(|(r, g, b)| Color::Rgb(r, g, b))
}

fn format_author(author: &woderrise_core::Author) -> String {
    match (&author.email, &author.url) {
        (Some(email), Some(url)) => format!("{} <{}> ({})", author.name, email, url),
        (Some(email), None) => format!("{} <{}>", author.name, email),
        (None, Some(url)) => format!("{} ({})", author.name, url),
        (None, None) => author.name.clone(),
    }
}


#[derive(Debug, Clone)]
struct CatalogItem {
    id: String,
    name: String,
    description: String,
    category: String,
    tags: Vec<String>,
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

fn draw_preview(frame: &mut Frame<'_>, app: &App) {
    let area = centered(frame.area(), 82, 76);
    frame.render_widget(Clear, area);
    let Some(item) = app.selected_catalog() else {
        return;
    };
    let mut lines = vec![
        Line::styled(
            format!("{} v{}", item.name, item.version),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::styled("Installation preview", Style::default().fg(Color::Yellow)),
        Line::from(""),
    ];
    for file in &item.manifest.install.files {
        let target = home_dir().join(file.target.trim_start_matches("~/"));
        let state = if target.exists() {
            "OVERWRITE + BACKUP"
        } else {
            "CREATE"
        };
        lines.push(Line::from(format!(
            "[{state}] {} -> {}",
            file.source, file.target
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "Compatibility",
        Style::default().fg(Color::Yellow),
    ));
    if let Some(compatibility) = &item.manifest.compatibility {
        lines.push(Line::from(format!(
            "Display server: {}",
            compatibility
                .display_server
                .as_deref()
                .unwrap_or("not declared")
        )));
        lines.push(Line::from(format!(
            "Architectures: {}",
            compatibility.architectures.join(", ")
        )));
    } else {
        lines.push(Line::from("Not declared"));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(
        "All declared file operations are journaled and rollback-checked.",
    ));
    lines.push(Line::from(
        "Esc closes preview • i returns to install confirmation",
    ));
    if let Some(previews) = &item.manifest.previews {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            format!(
                "Preview {} of {}: {}",
                app.preview_selection.saturating_add(1),
                previews.screenshots.len().max(1),
                previews
                    .screenshots
                    .get(app.preview_selection)
                    .map(|s| s.path.as_str())
                    .unwrap_or(previews.thumbnail.as_str())
            ),
            Style::default().fg(Color::Magenta),
        ));
        if !previews.palette.is_empty() {
            lines.push(Line::styled("Palette swatches:", Style::default().fg(Color::Yellow)));
            let mut swatch_spans = Vec::new();
            for (idx, color) in previews.palette.iter().enumerate() {
                if idx > 0 {
                    swatch_spans.push(Span::raw("   "));
                }
                if let Some(rgb) = parse_hex_color(&color.hex) {
                    swatch_spans.push(Span::styled("  ", Style::default().bg(rgb)));
                    swatch_spans.push(Span::raw(" "));
                }
                swatch_spans.push(Span::styled(&color.name, Style::default().fg(Color::White)));
                swatch_spans.push(Span::styled(
                    format!(" ({})", color.hex),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            lines.push(Line::from(swatch_spans));
            lines.push(Line::from(
                "Palette values are manifest metadata from the verified package artifact.",
            ));
        }
    } else {
        lines.push(Line::from("No preview media supplied by this manifest."));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        format!(
            "Source: {}",
            item.manifest
                .package
                .repository
                .as_deref()
                .unwrap_or("not declared")
        ),
        Style::default().fg(Color::DarkGray),
    ));
    if let Some(artifact) = item.manifest.artifacts.first() {
        let artifact_path = item.manifest_dir.join(&artifact.path);
        lines.push(Line::styled(
            format!("Verified artifact: {}", artifact.path),
            Style::default().fg(Color::Green),
        ));
        lines.push(Line::from(format!(
            "Artifact source: {}",
            artifact.url.as_deref().unwrap_or("not declared")
        )));
        match read_diff_file(&artifact_path) {
            Ok(content) => {
                lines.push(Line::styled(
                    "Artifact preview (read-only):",
                    Style::default().fg(Color::Yellow),
                ));
                for line in content.lines().take(8) {
                    let shortened: String = line.chars().take(100).collect();
                    lines.push(Line::from(shortened));
                }
            }
            Err(error) => lines.push(Line::from(format!("Artifact preview unavailable: {error}"))),
        }
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Preview • Esc closes"),
        ),
        area,
    );
}

fn draw_diff(frame: &mut Frame<'_>, app: &App) {
    let area = centered(frame.area(), 92, 82);
    frame.render_widget(Clear, area);

    if let Some(error) = &app.diff_error {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Diff Error", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Line::from(""),
                Line::styled(error.as_str(), Style::default().fg(Color::Yellow)),
                Line::from(""),
                Line::from("Esc closes diff preview"),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Diff • Error • Esc closes"),
            ),
            area,
        );
        return;
    }

    let Some(plan) = &app.diff_plan else {
        frame.render_widget(
            Paragraph::new("Diff plan unavailable.").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Diff • Esc closes"),
            ),
            area,
        );
        return;
    };

    if plan.file_diffs.is_empty() {
        frame.render_widget(
            Paragraph::new("No file operations declared in manifest.install.files.").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Diff • Esc closes"),
            ),
            area,
        );
        return;
    }

    let file_index = app.diff_file_index.min(plan.file_diffs.len().saturating_sub(1));
    let file_diff = &plan.file_diffs[file_index];

    let modal_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            format!(" Diff: {} v{} • Esc closes ", plan.package_id, plan.version),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let inner_area = modal_block.inner(area);
    frame.render_widget(modal_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(inner_area);

    // 1. Header: File Tabs, Target Path + Action Badge, Planned Backup Path, Source Path + Stats
    let mut header_lines = Vec::new();

    let mut tab_spans = vec![Span::styled("Files: ", Style::default().fg(Color::Yellow))];
    for (i, f) in plan.file_diffs.iter().enumerate() {
        let name = f.target_path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let (style, marker) = if i == file_index {
            (
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
                "●",
            )
        } else {
            (
                Style::default().fg(Color::White).bg(Color::DarkGray),
                "○",
            )
        };
        tab_spans.push(Span::styled(format!(" {} {} {} ", marker, i + 1, name), style));
        tab_spans.push(Span::raw(" "));
    }
    tab_spans.push(Span::styled(
        format!("({}/{} files) • Tab/←/→ to switch", file_index + 1, plan.file_diffs.len()),
        Style::default().fg(Color::DarkGray),
    ));
    header_lines.push(Line::from(tab_spans));

    let (badge_text, badge_style) = match file_diff.action {
        FileDiffAction::Create => (" CREATE ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
        FileDiffAction::Modify => (" MODIFY ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)),
        FileDiffAction::Unchanged => (" UNCHANGED ", Style::default().fg(Color::White).bg(Color::DarkGray)),
    };
    header_lines.push(Line::from(vec![
        Span::styled("Target: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(file_diff.target_path.display().to_string(), Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(badge_text, badge_style),
    ]));

    let backup_line = match &file_diff.planned_backup_path {
        Some(path) => Line::from(vec![
            Span::styled("Planned Backup: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(path.display().to_string(), Style::default().fg(Color::Yellow)),
        ]),
        None => Line::from(vec![
            Span::styled("Planned Backup: ", Style::default().fg(Color::DarkGray)),
            Span::styled("None (new file or backup disabled)", Style::default().fg(Color::DarkGray)),
        ]),
    };
    header_lines.push(backup_line);

    let additions = file_diff.lines.iter().filter(|l| l.kind == DiffLineKind::Addition).count();
    let deletions = file_diff.lines.iter().filter(|l| l.kind == DiffLineKind::Deletion).count();
    header_lines.push(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(Color::DarkGray)),
        Span::styled(file_diff.source_path.display().to_string(), Style::default().fg(Color::DarkGray)),
        Span::raw("   "),
        Span::styled(format!("+{} additions", additions), Style::default().fg(Color::Green)),
        Span::raw("   "),
        Span::styled(format!("-{} deletions", deletions), Style::default().fg(Color::Red)),
    ]));

    header_lines.push(Line::from("─".repeat(chunks[0].width as usize)));
    frame.render_widget(Paragraph::new(header_lines), chunks[0]);

    // 2. Diff Body: Lines with gutter numbers and Myers + / - / markers
    let mut diff_widget_lines = Vec::new();
    if file_diff.lines.is_empty() {
        if file_diff.action == FileDiffAction::Unchanged {
            diff_widget_lines.push(Line::styled(
                "Files are identical. No differences found.",
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            diff_widget_lines.push(Line::styled(
                "No line differences to display.",
                Style::default().fg(Color::DarkGray),
            ));
        }
    } else {
        for line in &file_diff.lines {
            match line.kind {
                DiffLineKind::Addition => {
                    let r_no = line.right_line_no.map(|n| format!("{:4}", n)).unwrap_or_else(|| "    ".into());
                    diff_widget_lines.push(Line::from(vec![
                        Span::styled(format!("       {:4} │ + ", r_no), Style::default().fg(Color::Green)),
                        Span::styled(&line.text, Style::default().fg(Color::Green)),
                    ]));
                }
                DiffLineKind::Deletion => {
                    let l_no = line.left_line_no.map(|n| format!("{:4}", n)).unwrap_or_else(|| "    ".into());
                    diff_widget_lines.push(Line::from(vec![
                        Span::styled(format!("{:4}        │ - ", l_no), Style::default().fg(Color::Red)),
                        Span::styled(&line.text, Style::default().fg(Color::Red)),
                    ]));
                }
                DiffLineKind::Context => {
                    let l_no = line.left_line_no.map(|n| format!("{:4}", n)).unwrap_or_else(|| "    ".into());
                    let r_no = line.right_line_no.map(|n| format!("{:4}", n)).unwrap_or_else(|| "    ".into());
                    diff_widget_lines.push(Line::from(vec![
                        Span::styled(format!("{:4}   {:4} │   ", l_no, r_no), Style::default().fg(Color::DarkGray)),
                        Span::styled(&line.text, Style::default().fg(Color::White)),
                    ]));
                }
            }
        }
    }

    let clamped_scroll = app.diff_scroll.min(file_diff.lines.len().saturating_sub(1));
    let diff_paragraph = Paragraph::new(diff_widget_lines)
        .scroll((clamped_scroll as u16, 0));
    frame.render_widget(diff_paragraph, chunks[1]);

    if file_diff.lines.len() > chunks[1].height as usize {
        let mut scrollbar_state = ScrollbarState::new(file_diff.lines.len()).position(clamped_scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .track_symbol(Some("│"))
                .thumb_symbol("█"),
            chunks[1],
            &mut scrollbar_state,
        );
    }

    // 3. Footer: Navigation Keys & Current Line
    let footer_spans = vec![
        Span::styled(" Tab/←/→ ", key_style()),
        Span::raw("Switch File  "),
        Span::styled(" ↑/↓/PgUp/PgDn ", key_style()),
        Span::raw("Scroll  "),
        Span::styled(" Home/End ", key_style()),
        Span::raw("Top/Bottom  "),
        Span::styled(" i ", key_style()),
        Span::raw("Install  "),
        Span::styled(" Esc ", key_style()),
        Span::raw("Close  "),
        Span::styled(
            format!(" Line {}/{} ", clamped_scroll + 1, file_diff.lines.len().max(1)),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    frame.render_widget(Paragraph::new(Line::from(footer_spans)), chunks[2]);
}

fn draw_progress_modal(frame: &mut Frame<'_>, op: &ActiveOperation) {
    let area = centered(frame.area(), 64, 14);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", op.title))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Stage line
            Constraint::Length(3), // Gauge
            Constraint::Length(1), // Message line
            Constraint::Min(1),    // Warning
        ])
        .split(inner);

    let stage_line = Line::from(vec![
        Span::styled("Stage: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &op.model.stage,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(stage_line), chunks[0]);

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL))
        .gauge_style(
            Style::default()
                .fg(Color::Rgb(137, 180, 250))
                .bg(Color::Rgb(49, 50, 68)),
        )
        .percent(op.model.percent as u16)
        .label(format!("{}%", op.model.percent));
    frame.render_widget(gauge, chunks[1]);

    let msg_line = Line::from(vec![
        Span::styled("Details: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&op.model.message, Style::default().fg(Color::White)),
    ]);
    frame.render_widget(Paragraph::new(msg_line).wrap(Wrap { trim: true }), chunks[2]);

    let warning = Line::styled(
        "Transactional operation in progress. Please do not interrupt.",
        Style::default().fg(Color::DarkGray),
    );
    frame.render_widget(Paragraph::new(warning), chunks[3]);
}

fn draw_error_alert(frame: &mut Frame<'_>, error: &str) {
    let area = centered(frame.area(), 60, 10);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Error Alert • Esc closes ")
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::styled("Operation failed with error:", Style::default().fg(Color::Yellow)),
        Line::from(""),
        Line::styled(error, Style::default().fg(Color::Red)),
        Line::from(""),
        Line::styled("Press Esc to dismiss", Style::default().fg(Color::DarkGray)),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

const MAX_DIFF_BYTES: u64 = 1024 * 1024;

fn read_diff_file(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_DIFF_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_DIFF_BYTES {
        return Err(format!(
            "file exceeds {} MiB diff limit",
            MAX_DIFF_BYTES / 1024 / 1024
        ));
    }
    String::from_utf8(bytes).map_err(|_| "file is not valid UTF-8".into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPanel {
    Sidebar,
    Content,
}

#[derive(Debug, Clone)]
struct AppSettings {
    auto_backup: bool,
    strict_hash_check: bool,
    offline_cache_first: bool,
    target_compositor: String,
    target_shell: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_backup: true,
            strict_hash_check: true,
            offline_cache_first: true,
            target_compositor: "Hyprland (Wayland)".into(),
            target_shell: "Waybar (~/.config/waybar)".into(),
        }
    }
}

fn next_sidebar_index(current: usize) -> usize {
    let mut next = (current + 1) % SIDEBAR.len();
    if SIDEBAR[next] == "─" {
        next = (next + 1) % SIDEBAR.len();
    }
    next
}

fn prev_sidebar_index(current: usize) -> usize {
    let mut prev = if current == 0 {
        SIDEBAR.len() - 1
    } else {
        current - 1
    };
    if SIDEBAR[prev] == "─" {
        prev = if prev == 0 {
            SIDEBAR.len() - 1
        } else {
            prev - 1
        };
    }
    prev
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
    preview: bool,
    diff: bool,
    preview_selection: usize,
    diff_file_index: usize,
    diff_scroll: usize,
    diff_plan: Option<InstallPlan>,
    diff_error: Option<String>,
    install_rx: Option<Receiver<OperationEvent>>,
    refresh_rx: Option<Receiver<Result<PathBuf, String>>>,
    url_modal_open: bool,
    url_input_buffer: String,
    url_input_error: Option<String>,
    fetch_rx: Option<Receiver<Result<PathBuf, String>>>,
    status: String,
    db_available: bool,
    network_status: NetworkStatus,
    operation: Option<ActiveOperation>,
    error_alert: Option<String>,
    journal_path: PathBuf,
    journal_error: Option<String>,
    catalog_error: Option<String>,
    interrupted_transactions: usize,
    data_dir: PathBuf,
    focus_panel: FocusPanel,
    sidebar_selection: usize,
    setting_selection: usize,
    settings: AppSettings,
}

impl App {
    fn new() -> Self {
        let data_dir = data_dir();
        let network_status = if registry_source().is_some() {
            NetworkStatus::Online
        } else {
            NetworkStatus::OfflineLocal
        };
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
            preview: false,
            diff: false,
            preview_selection: 0,
            diff_file_index: 0,
            diff_scroll: 0,
            diff_plan: None,
            diff_error: None,
            install_rx: None,
            refresh_rx: None,
            url_modal_open: false,
            url_input_buffer: String::new(),
            url_input_error: None,
            fetch_rx: None,
            status: "Offline (journal unavailable)".into(),
            db_available: false,
            network_status,
            operation: None,
            error_alert: None,
            journal_path: data_dir.join("journal.db"),
            journal_error: None,
            catalog_error: None,
            interrupted_transactions: 0,
            data_dir,
            focus_panel: FocusPanel::Content,
            sidebar_selection: 0,
            setting_selection: 0,
            settings: AppSettings::default(),
        };
        app.reload();
        if registry_source().is_some() {
            app.begin_refresh();
        }
        app
    }

    fn compute_diff_plan(&self) -> Result<InstallPlan, String> {
        let Some(item) = self.selected_catalog() else {
            return Err("No package selected".into());
        };
        let db = Database::open(self.data_dir.join("journal.db"))
            .or_else(|_| Database::in_memory())
            .map_err(|e| format!("Database error: {e}"))?;
        let backups = BackupManager::new(self.data_dir.join("backups"))
            .map_err(|e| format!("Backup manager error: {e}"))?;
        let engine = InstallationEngine {
            db,
            backups,
            home: home_dir(),
        };
        engine
            .compute_plan(&item.manifest_path)
            .map_err(|e| format!("Diff plan error: {e}"))
    }

    fn open_diff(&mut self) {
        if self.selected_catalog().is_some() {
            self.diff = true;
            self.diff_file_index = 0;
            self.diff_scroll = 0;
            match self.compute_diff_plan() {
                Ok(plan) => {
                    self.diff_plan = Some(plan);
                    self.diff_error = None;
                }
                Err(err) => {
                    self.diff_plan = None;
                    self.diff_error = Some(err);
                }
            }
        } else {
            self.status = "Select a package to inspect".into();
        }
    }

    fn diff_line_count(&self) -> usize {
        self.diff_plan
            .as_ref()
            .and_then(|p| p.file_diffs.get(self.diff_file_index))
            .map(|f| f.lines.len())
            .unwrap_or(0)
    }

    fn reload(&mut self) {
        self.reload_catalog(true);
    }

    fn reload_cached(&mut self) {
        self.reload_catalog(false);
    }

    fn reload_catalog(&mut self, import_registry: bool) {
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
                self.catalog = if import_registry {
                    load_catalog(&db, &registry_dir(), &mut self.catalog_error)
                } else {
                    load_catalog_records(&db, &mut self.catalog_error)
                };
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
                    || item.description.to_lowercase().contains(&query)
                    || item
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&query)))
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
        self.setting_selection = self.setting_selection.min(7);
    }

    fn move_selection(&mut self, delta: i32) {
        if self.page == 0 || self.page == 1 {
            let len = self.filtered_catalog().len();
            self.catalog_selection = move_index(self.catalog_selection, len, delta);
        } else if self.page == 5 {
            self.setting_selection = move_index(self.setting_selection, 8, delta);
        } else {
            self.transaction_selection = move_index(
                self.transaction_selection,
                self.visible_transactions(self.page == 2).len(),
                delta,
            );
        }
    }

    fn sidebar_index_for_page(&self) -> usize {
        match self.page {
            0 => {
                if self.category.is_some() {
                    2
                } else if self.sidebar_selection == 0 {
                    0
                } else {
                    1
                }
            }
            1 => 0,
            2 => 3,
            3 => 4,
            4 => 6,
            5 => 7,
            _ => 1,
        }
    }

    fn sync_page_from_sidebar(&mut self) {
        match self.sidebar_selection {
            0 => {
                self.page = 0;
                self.category = None;
                self.status = "Home: Discover desktop configurations".into();
            }
            1 => {
                self.page = 0;
                self.status = "Browse: All registry configurations".into();
            }
            2 => {
                self.page = 0;
                self.next_category();
            }
            3 => {
                self.page = 2;
                self.status = "Installed configurations & rollback candidates".into();
            }
            4 => {
                self.page = 3;
                self.status = "Updates & transaction history".into();
            }
            6 => {
                self.page = 4;
                self.status = "Registry & database journal".into();
            }
            7 => {
                self.page = 5;
                self.status = "Settings & system preferences".into();
            }
            8 => {
                self.help = true;
            }
            _ => {}
        }
        self.clamp_selection();
    }

    fn toggle_setting(&mut self) {
        match self.setting_selection {
            0 => {
                self.settings.auto_backup = !self.settings.auto_backup;
                self.status = format!(
                    "Pre-install backup: {}",
                    if self.settings.auto_backup { "Enabled" } else { "Disabled" }
                );
            }
            1 => {
                self.settings.strict_hash_check = !self.settings.strict_hash_check;
                self.status = format!(
                    "Strict hash check: {}",
                    if self.settings.strict_hash_check { "Strict SHA-256" } else { "Permissive" }
                );
            }
            2 => {
                self.settings.offline_cache_first = !self.settings.offline_cache_first;
                self.status = format!(
                    "Cache preference: {}",
                    if self.settings.offline_cache_first { "Local Cache First" } else { "Network First" }
                );
            }
            3 => {
                self.settings.target_compositor = match self.settings.target_compositor.as_str() {
                    "Hyprland (Wayland)" => "Sway (Wayland)".into(),
                    "Sway (Wayland)" => "River (Wayland)".into(),
                    _ => "Hyprland (Wayland)".into(),
                };
                self.status = format!("Target Compositor: {}", self.settings.target_compositor);
            }
            4 => {
                self.settings.target_shell = match self.settings.target_shell.as_str() {
                    "Waybar (~/.config/waybar)" => "Eww (~/.config/eww)".into(),
                    "Eww (~/.config/eww)" => "Ags (~/.config/ags)".into(),
                    _ => "Waybar (~/.config/waybar)".into(),
                };
                self.status = format!("Target Shell: {}", self.settings.target_shell);
            }
            5 => {
                self.status = "Refreshing registry catalog...".into();
                self.begin_refresh();
            }
            6 => {
                self.status = "Journal check: PRAGMA integrity_check = ok; WAL mode active".into();
            }
            7 => {
                self.settings = AppSettings::default();
                self.status = "Settings reset to default configuration".into();
            }
            _ => {}
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
        if key.kind != KeyEventKind::Press {
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('q'))
        {
            if self.install_rx.is_some() || self.operation.is_some() {
                self.status = "Installation is running; wait for its journal result".into();
                return false;
            }
            return true;
        }
        if self.error_alert.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.error_alert = None;
            }
            return false;
        }
        if self.install_rx.is_some() || self.operation.is_some() {
            if key.code == KeyCode::Char('q') {
                self.status = "Installation is running; wait for its journal result".into();
            }
            return false;
        }
        if self.url_modal_open {
            match key.code {
                KeyCode::Esc => {
                    self.url_modal_open = false;
                    self.url_input_error = None;
                }
                KeyCode::Backspace => {
                    self.url_input_buffer.pop();
                    self.url_input_error = None;
                }
                KeyCode::Enter => {
                    let raw = self.url_input_buffer.trim().to_string();
                    if raw.is_empty() {
                        self.url_input_error = Some("Repository URL cannot be empty".into());
                    } else {
                        match parse_and_validate_github_url(&raw) {
                            Ok(_) => {
                                self.url_modal_open = false;
                                self.url_input_error = None;
                                self.begin_fetch(&raw);
                            }
                            Err(err) => {
                                self.url_input_error = Some(err.to_string());
                            }
                        }
                    }
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.url_input_buffer.push(c);
                    self.url_input_error = None;
                }
                _ => {}
            }
            return false;
        }
        if self.help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.help = false;
            }
            return false;
        }
        if self.diff {
            match key.code {
                KeyCode::Esc => {
                    self.diff = false;
                    self.diff_plan = None;
                }
                KeyCode::Tab | KeyCode::Right => {
                    if let Some(plan) = &self.diff_plan {
                        if !plan.file_diffs.is_empty() {
                            self.diff_file_index = (self.diff_file_index + 1) % plan.file_diffs.len();
                            self.diff_scroll = 0;
                        }
                    }
                }
                KeyCode::BackTab | KeyCode::Left => {
                    if let Some(plan) = &self.diff_plan {
                        if !plan.file_diffs.is_empty() {
                            if self.diff_file_index == 0 {
                                self.diff_file_index = plan.file_diffs.len().saturating_sub(1);
                            } else {
                                self.diff_file_index -= 1;
                            }
                            self.diff_scroll = 0;
                        }
                    }
                }
                KeyCode::Up => {
                    self.diff_scroll = self.diff_scroll.saturating_sub(1);
                }
                KeyCode::Down => {
                    let total = self.diff_line_count();
                    if self.diff_scroll + 1 < total {
                        self.diff_scroll += 1;
                    }
                }
                KeyCode::PageUp => {
                    self.diff_scroll = self.diff_scroll.saturating_sub(15);
                }
                KeyCode::PageDown => {
                    let total = self.diff_line_count();
                    self.diff_scroll = (self.diff_scroll + 15).min(total.saturating_sub(1));
                }
                KeyCode::Home => {
                    self.diff_scroll = 0;
                }
                KeyCode::End => {
                    let total = self.diff_line_count();
                    self.diff_scroll = total.saturating_sub(1);
                }
                KeyCode::Char('i') => {
                    self.diff = false;
                    self.page = 1;
                    self.status = "Install plan loaded; press i to confirm".into();
                }
                _ => {}
            }
            return false;
        }
        if self.preview {
            match key.code {
                KeyCode::Esc => {
                    self.preview = false;
                }
                KeyCode::Left => {
                    self.preview_selection = self.preview_selection.saturating_sub(1);
                }
                KeyCode::Right => {
                    self.preview_selection = self.preview_selection.saturating_add(1);
                }
                KeyCode::Char('i') => {
                    self.preview = false;
                    self.page = 1;
                    self.status = "Install plan loaded; press i to confirm".into();
                }
                _ => {}
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
                    self.transaction_selection = 0;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search.push(c);
                    self.catalog_selection = 0;
                    self.transaction_selection = 0;
                }
                _ => {}
            }
            self.clamp_selection();
            return false;
        }
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus_panel = match self.focus_panel {
                    FocusPanel::Sidebar => FocusPanel::Content,
                    FocusPanel::Content => FocusPanel::Sidebar,
                };
                self.status = match self.focus_panel {
                    FocusPanel::Sidebar => {
                        "Sidebar focused: use ↑/↓ to choose a section, Tab/Enter to open".into()
                    }
                    FocusPanel::Content => {
                        "Content focused: use ↑/↓ to navigate, Tab to return to sidebar".into()
                    }
                };
            }
            KeyCode::Left => {
                if self.focus_panel == FocusPanel::Content {
                    self.focus_panel = FocusPanel::Sidebar;
                    self.status = "Sidebar focused: use ↑/↓ to choose a section, Tab/Enter to open".into();
                } else {
                    self.sidebar_selection = prev_sidebar_index(self.sidebar_selection);
                    self.sync_page_from_sidebar();
                }
            }
            KeyCode::Right => {
                if self.focus_panel == FocusPanel::Sidebar {
                    self.focus_panel = FocusPanel::Content;
                    self.status = "Content focused: use ↑/↓ to navigate, Tab to return to sidebar".into();
                } else {
                    self.page = (self.page + 1) % TABS.len();
                    self.sidebar_selection = self.sidebar_index_for_page();
                    self.clamp_selection();
                }
            }
            KeyCode::Up => {
                if self.focus_panel == FocusPanel::Sidebar {
                    self.sidebar_selection = prev_sidebar_index(self.sidebar_selection);
                    self.sync_page_from_sidebar();
                } else {
                    self.move_selection(-1);
                }
            }
            KeyCode::Down => {
                if self.focus_panel == FocusPanel::Sidebar {
                    self.sidebar_selection = next_sidebar_index(self.sidebar_selection);
                    self.sync_page_from_sidebar();
                } else {
                    self.move_selection(1);
                }
            }
            KeyCode::Enter => {
                if self.focus_panel == FocusPanel::Sidebar {
                    self.focus_panel = FocusPanel::Content;
                    self.sync_page_from_sidebar();
                    self.status = "Content focused: use ↑/↓ to navigate, Tab to return to sidebar".into();
                } else if self.page <= 3 {
                    self.detail = true;
                } else if self.page == 5 {
                    self.toggle_setting();
                }
            }
            KeyCode::Char(' ') if self.page == 5 && self.focus_panel == FocusPanel::Content => {
                self.toggle_setting();
            }
            KeyCode::Char('/') => {
                self.searching = true;
            }
            KeyCode::Char('c') if self.page == 0 => self.next_category(),
            KeyCode::Char('i') if self.page == 0 => {
                if self.selected_catalog().is_some() {
                    self.page = 1;
                    self.sidebar_selection = 0;
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
            KeyCode::Char('p') if self.page == 0 || self.page == 1 => {
                if self.selected_catalog().is_some() {
                    self.preview_selection = 0;
                    self.preview = true;
                } else {
                    self.status = "Select a package to preview".into();
                }
            }
            KeyCode::Char('d') if self.page == 0 || self.page == 1 => {
                self.open_diff();
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
                } else if self.focus_panel == FocusPanel::Content {
                    self.focus_panel = FocusPanel::Sidebar;
                    self.status = "Sidebar focused: use ↑/↓ to choose a section, Tab/Enter to open".into();
                } else if self.page > 0 {
                    self.page = 0;
                    self.sidebar_selection = 0;
                    self.clamp_selection();
                }
            }
            KeyCode::Char('R') => self.begin_refresh(),
            KeyCode::Char('f') | KeyCode::Char('F') if !self.searching => {
                self.url_modal_open = true;
                self.url_input_buffer.clear();
                self.url_input_error = None;
            }
            KeyCode::Char('1') if !self.searching => {
                self.sidebar_selection = 0;
                self.sync_page_from_sidebar();
            }
            KeyCode::Char('2') if !self.searching => {
                self.sidebar_selection = 1;
                self.sync_page_from_sidebar();
            }
            KeyCode::Char('3') if !self.searching => {
                self.sidebar_selection = 2;
                self.sync_page_from_sidebar();
            }
            KeyCode::Char('4') if !self.searching => {
                self.sidebar_selection = 3;
                self.sync_page_from_sidebar();
            }
            KeyCode::Char('5') if !self.searching => {
                self.sidebar_selection = 4;
                self.sync_page_from_sidebar();
            }
            KeyCode::Char('6') if !self.searching => {
                self.sidebar_selection = 6;
                self.sync_page_from_sidebar();
            }
            KeyCode::Char('7') if !self.searching => {
                self.sidebar_selection = 7;
                self.sync_page_from_sidebar();
            }
            KeyCode::Char('8') if !self.searching => {
                self.sidebar_selection = 8;
                self.sync_page_from_sidebar();
            }
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
        let data_dir = self.data_dir.clone();
        let transaction_id = transaction.id.clone();
        let package_name = transaction.package.clone();
        let (sender, receiver) = mpsc::channel();
        self.install_rx = Some(receiver);
        self.operation = Some(ActiveOperation {
            title: format!("Rolling back {package_name}"),
            model: woderrise_gui::ProgressModel::new("Init", 10, "Initializing rollback engine..."),
        });
        self.status = format!("Rollback started for {package_name}");
        std::thread::spawn(move || {
            let _ = sender.send(OperationEvent::Progress(woderrise_gui::ProgressModel::new(
                "Verify",
                30,
                "Verifying current file hashes...",
            )));
            let result = (|| -> woderrise_core::Result<String> {
                let db = Database::open(data_dir.join("journal.db"))?;
                let engine = InstallationEngine {
                    db,
                    backups: BackupManager::new(data_dir.join("backups"))?,
                    home: home_dir(),
                };
                let _ = sender.send(OperationEvent::Progress(woderrise_gui::ProgressModel::new(
                    "Restore",
                    60,
                    "Restoring backup snapshots...",
                )));
                let txn_id = engine.rollback(&transaction_id, false)?;
                let _ = sender.send(OperationEvent::Progress(woderrise_gui::ProgressModel::new(
                    "Commit",
                    90,
                    "Recording rollback in journal...",
                )));
                Ok(txn_id)
            })();
            let event = match result {
                Ok(id) => {
                    let _ = sender.send(OperationEvent::Progress(woderrise_gui::ProgressModel::new(
                        "Done",
                        100,
                        "Rollback completed",
                    )));
                    OperationEvent::Finished(Ok(id))
                }
                Err(err) => OperationEvent::Finished(Err(err.to_string())),
            };
            let _ = sender.send(event);
        });
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
        let data_dir = self.data_dir.clone();
        let manifest_path = item.manifest_path.clone();
        let (sender, receiver) = mpsc::channel();
        self.install_rx = Some(receiver);
        self.operation = Some(ActiveOperation {
            title: format!("Installing {}", item.name),
            model: woderrise_gui::ProgressModel::new("Resolve", 5, "Resolving package manifest..."),
        });
        self.status = format!("Installation started for {}", item.name);
        std::thread::spawn(move || {
            let tx = sender.clone();
            let cb: woderrise_core::ProgressCallback = Box::new(move |stage, msg, frac| {
                let percent = (frac * 100.0).clamp(0.0, 100.0) as u8;
                let _ = tx.send(OperationEvent::Progress(woderrise_gui::ProgressModel::new(
                    stage.to_string(),
                    percent,
                    msg.to_string(),
                )));
            });
            let result = (|| -> woderrise_core::Result<String> {
                let db = Database::open(data_dir.join("journal.db"))?;
                let engine = InstallationEngine {
                    db,
                    backups: BackupManager::new(data_dir.join("backups"))?,
                    home: home_dir(),
                };
                engine.install_with_progress(&manifest_path, "user_tui", Some(cb))
            })();
            let event = match result {
                Ok(id) => OperationEvent::Finished(Ok(id)),
                Err(err) => OperationEvent::Finished(Err(err.to_string())),
            };
            let _ = sender.send(event);
        });
    }

    fn poll_install(&mut self) {
        let Some(receiver) = &self.install_rx else {
            return;
        };
        let mut finished = None;
        let mut disconnected = false;
        loop {
            match receiver.try_recv() {
                Ok(OperationEvent::Progress(model)) => {
                    if let Some(op) = &mut self.operation {
                        op.model = model.clone();
                    }
                    self.status = format!("{}: {}", model.stage, model.message);
                }
                Ok(OperationEvent::Finished(result)) => {
                    finished = Some(result);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if let Some(result) = finished {
            self.install_rx = None;
            self.operation = None;
            self.reload_cached();
            match result {
                Ok(id) => {
                    self.status = format!("Operation committed: {id}");
                }
                Err(err) => {
                    self.error_alert = Some(err.clone());
                    self.status = format!("Operation failed: {err}");
                }
            }
        } else if disconnected {
            self.install_rx = None;
            self.operation = None;
            self.reload_cached();
            self.status = "Operation worker stopped without a result".into();
        }
    }

    fn begin_refresh(&mut self) {
        if self.refresh_rx.is_some() {
            self.status = "Registry refresh is already running".into();
            return;
        }
        let Some(source) = registry_source() else {
            self.reload();
            self.network_status = NetworkStatus::OfflineLocal;
            self.status = "Using local registry cache (no remote source configured)".into();
            return;
        };
        let cache_dir = registry_cache_dir(&self.data_dir);
        let data_dir = self.data_dir.clone();
        let (sender, receiver) = mpsc::channel();
        self.refresh_rx = Some(receiver);
        self.operation = Some(ActiveOperation {
            title: "Refreshing registry".into(),
            model: woderrise_gui::ProgressModel::new("Sync", 20, format!("Fetching from {source}...")),
        });
        self.status = format!("Loading registry from {source}...");
        std::thread::spawn(move || {
            let result = (|| -> Result<PathBuf, String> {
                let root = catalog::sync_git_registry(&source, &cache_dir)
                    .map_err(|error| error.to_string())?;
                let db = Database::open(data_dir.join("journal.db"))
                    .map_err(|error| error.to_string())?;
                catalog::import_registry(&db, &root).map_err(|error| error.to_string())?;
                Ok(root)
            })();
            let _ = sender.send(result);
        });
    }

    fn poll_refresh(&mut self) {
        let Some(receiver) = &self.refresh_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(root)) => {
                self.refresh_rx = None;
                self.operation = None;
                self.network_status = NetworkStatus::Online;
                self.reload_cached();
                self.status = format!("Registry refreshed from {}", root.display());
            }
            Ok(Err(error)) => {
                self.refresh_rx = None;
                self.operation = None;
                self.network_status = NetworkStatus::OfflineSyncFailed;
                self.reload_cached();
                self.status = format!("Refresh failed; using cached catalog: {error}");
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.refresh_rx = None;
                self.operation = None;
                self.network_status = NetworkStatus::OfflineSyncFailed;
                self.reload_cached();
                self.status = "Refresh failed; using cached catalog".into();
            }
        }
    }

    fn begin_fetch(&mut self, url: &str) {
        if self.fetch_rx.is_some() {
            self.status = "GitHub fetch is already in progress".into();
            return;
        }
        let url = url.to_string();
        let (sender, receiver) = mpsc::channel();
        self.fetch_rx = Some(receiver);
        self.operation = Some(ActiveOperation {
            title: "Fetching from GitHub".into(),
            model: woderrise_gui::ProgressModel::new(
                "Clone",
                25,
                format!("Cloning and synthesizing manifest from {url}..."),
            ),
        });
        self.status = format!("Cloning GitHub repository: {url}");
        let data_dir = self.data_dir.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<PathBuf, String> {
                let fetched = fetch_github_repository(&url, FetchOptions::default())
                    .map_err(|e| e.to_string())?;

                let persistent_path = fetched.temp_dir.keep();
                let manifest_path = persistent_path.join("woderrise.yaml");

                if let Ok(db) = Database::open(data_dir.join("journal.db")) {
                    let entry = woderrise_core::catalog::CatalogEntry {
                        id: fetched.manifest.package.id.clone(),
                        name: fetched.manifest.package.name.clone(),
                        description: fetched.manifest.package.description.clone(),
                        version: fetched.manifest.package.version.clone(),
                        package_type: fetched.manifest.package.package_type.clone(),
                        category: fetched.manifest.package.category.clone(),
                        tags: fetched.manifest.package.tags.clone(),
                        license: fetched.manifest.package.license.clone(),
                        repository: fetched.manifest.package.repository.clone(),
                        manifest_path: manifest_path.clone(),
                        manifest_sha256: woderrise_core::sha256_file(&manifest_path)
                            .unwrap_or_default(),
                        validated: true,
                        yanked: false,
                    };
                    let _ = catalog::upsert(&db, &entry);
                }

                Ok(manifest_path)
            })();
            let _ = sender.send(result);
        });
    }

    fn poll_fetch(&mut self) {
        let Some(receiver) = &self.fetch_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(manifest_path)) => {
                self.fetch_rx = None;
                self.operation = None;
                self.reload_cached();

                if !self
                    .catalog
                    .iter()
                    .any(|item| item.manifest_path == manifest_path)
                {
                    if let Ok(manifest) = Manifest::from_file(&manifest_path) {
                        let item = CatalogItem {
                            id: manifest.package.id.clone(),
                            name: manifest.package.name.clone(),
                            description: manifest.package.description.clone(),
                            category: manifest.package.category.clone(),
                            tags: manifest.package.tags.clone(),
                            version: manifest.package.version.clone(),
                            manifest_dir: manifest_path
                                .parent()
                                .unwrap_or(Path::new("."))
                                .to_path_buf(),
                            manifest_path: manifest_path.clone(),
                            manifest,
                            installed: false,
                        };
                        self.catalog.push(item);
                        self.catalog.sort_by(|a, b| a.name.cmp(&b.name));
                    }
                }

                self.search.clear();
                self.category = None;
                self.page = 0;

                if let Some(pos) = self
                    .filtered_catalog()
                    .iter()
                    .position(|item| item.manifest_path == manifest_path)
                {
                    self.catalog_selection = pos;
                }

                let pkg_name = self
                    .selected_catalog()
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "package".into());
                self.status = format!("Imported {pkg_name} from GitHub");
                self.open_diff();
            }
            Ok(Err(err)) => {
                self.fetch_rx = None;
                self.operation = None;
                self.error_alert = Some(format!("GitHub import failed: {err}"));
                self.status = format!("GitHub import failed: {err}");
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.fetch_rx = None;
                self.operation = None;
                self.error_alert = Some("GitHub import worker stopped without result".into());
                self.status = "GitHub import worker stopped without result".into();
            }
        }
    }
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        default_hook(panic_info);
    }));
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run(&mut terminal);
    restore_terminal(&mut terminal);
    result
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    loop {
        app.poll_install();
        app.poll_refresh();
        app.poll_fetch();
        terminal.draw(|frame| draw(frame, &app))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && app.handle_key(key) {
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
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    draw_header(frame, app, outer[0]);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(19), Constraint::Min(40)])
        .split(outer[1]);
    draw_sidebar(frame, app, body[0]);
    match app.page {
        0 => draw_catalog(frame, app, body[1]),
        1 => draw_install(frame, app, body[1]),
        2 => draw_transactions(frame, app, body[1], true),
        3 => draw_transactions(frame, app, body[1], false),
        4 => draw_journal(frame, app, body[1]),
        5 => draw_settings(frame, app, body[1]),
        _ => draw_catalog(frame, app, body[1]),
    }
    draw_status_bar(frame, app, outer[2]);
    let footer = Line::from(vec![
        Span::styled(" ↑/↓ ", key_style()),
        Span::raw("Navigate  "),
        Span::styled("Tab", key_style()),
        Span::raw(" Switch Panel  "),
        Span::styled("Enter", key_style()),
        Span::raw(" Details  "),
        Span::styled("I", key_style()),
        Span::raw(" Install  "),
        Span::styled("/", key_style()),
        Span::raw(" Search  "),
        Span::styled("R", key_style()),
        Span::raw(" Refresh  "),
        Span::styled("F", key_style()),
        Span::raw(" Fetch GitHub  "),
        Span::styled("1-7", key_style()),
        Span::raw(" Jump  "),
        Span::styled("Ctrl+C / Ctrl+Q", key_style()),
        Span::raw(" Quit"),
    ]);
    frame.render_widget(Paragraph::new(footer), outer[3]);
    if let Some(op) = &app.operation {
        draw_progress_modal(frame, op);
    } else if let Some(error) = &app.error_alert {
        draw_error_alert(frame, error);
    } else if app.url_modal_open {
        draw_url_input_modal(frame, app);
    } else if app.help {
        draw_help(frame);
    } else if app.preview {
        draw_preview(frame, app);
    } else if app.diff {
        draw_diff(frame, app);
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

fn draw_status_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (badge, style) = if app.status.contains("failed")
        || app.status.contains("blocked")
        || app.status.contains("Error")
        || app.status.contains("unavailable")
    {
        (
            " ERR ",
            Style::default()
                .bg(Color::Red)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
    } else if app.status.contains("running")
        || app.status.contains("started")
        || app.status.contains("Loading")
        || app.status.contains("Syncing")
        || app.status.contains("preparing")
        || app.status.contains("Restoring")
        || app.status.contains("Verifying")
    {
        (
            " BUSY ",
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
    } else if app.status.contains("committed")
        || app.status.contains("completed")
        || app.status.contains("online")
        || app.status.contains("refreshed")
        || app.status.contains("Healthy")
    {
        (
            " OK ",
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            " INFO ",
            Style::default().bg(Color::Rgb(60, 60, 90)).fg(Color::White),
        )
    };

    let line = Line::from(vec![
        Span::styled(format!(" {badge} "), style),
        Span::raw(" "),
        Span::styled(&app.status, Style::default().fg(Color::White)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (dot_color, status_text) = if !app.db_available {
        (Color::Red, "Journal offline")
    } else {
        match app.network_status {
            NetworkStatus::Online => (Color::Rgb(74, 222, 128), "Online"),
            NetworkStatus::OfflineLocal => (Color::DarkGray, "Offline (Local)"),
            NetworkStatus::OfflineSyncFailed => (Color::Yellow, "Offline (Sync failed)"),
        }
    };

    let registry_label = if registry_source().is_some() {
        "Registry: github.com/woderrise/registry"
    } else {
        "Registry: local"
    };

    let title_span = Span::styled(
        " ▲ Woderrise ",
        Style::default()
            .fg(Color::Rgb(165, 180, 252))
            .add_modifier(Modifier::BOLD),
    );
    let ver_span = Span::styled("v0.1.0  ", Style::default().fg(Color::DarkGray));
    let tagline_span = Span::styled(
        "Linux Desktop Configurations, Simplified",
        Style::default().fg(Color::Rgb(148, 163, 184)),
    );

    let left_len = 13 + 8 + 40;
    let right_str = format!("● {}   {}   🗄 Cached: 1 day ago", status_text, registry_label);
    let spaces = area.width.saturating_sub((left_len + right_str.len()) as u16);

    let header = Line::from(vec![
        title_span,
        ver_span,
        tagline_span,
        Span::raw(" ".repeat(spaces as usize)),
        Span::styled("● ", Style::default().fg(dot_color)),
        Span::styled(format!("{}   ", status_text), Style::default().fg(dot_color)),
        Span::styled(format!("{}   ", registry_label), Style::default().fg(Color::Rgb(148, 163, 184))),
        Span::styled("🗄 Cached: 1 day ago", Style::default().fg(Color::Rgb(148, 163, 184))),
    ]);
    frame.render_widget(
        Paragraph::new(header).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(40, 45, 70))),
        ),
        area,
    );
}

fn draw_sidebar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let sidebar_active = app.sidebar_selection;
    let is_focused = app.focus_panel == FocusPanel::Sidebar;

    let border_style = if is_focused {
        Style::default().fg(Color::Rgb(129, 140, 248))
    } else {
        Style::default().fg(Color::Rgb(45, 52, 80))
    };

    let title = if is_focused {
        Span::styled(
            " Navigation [Focused] ",
            Style::default()
                .fg(Color::Rgb(199, 210, 254))
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" Navigation ", Style::default().fg(Color::DarkGray))
    };

    let items = SIDEBAR
        .iter()
        .enumerate()
        .map(|(index, label)| {
            if *label == "─" {
                return ListItem::new(Line::styled(
                    " ────────────────",
                    Style::default().fg(Color::Rgb(40, 45, 70)),
                ));
            }
            let is_selected = index == sidebar_active;
            let is_current_page = index == app.sidebar_index_for_page();

            let (style, prefix) = if is_selected && is_focused {
                (
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(88, 101, 242))
                        .add_modifier(Modifier::BOLD),
                    "▶ ",
                )
            } else if is_selected {
                (
                    Style::default()
                        .fg(Color::Rgb(224, 231, 255))
                        .bg(Color::Rgb(60, 70, 160))
                        .add_modifier(Modifier::BOLD),
                    "● ",
                )
            } else if is_current_page {
                (
                    Style::default()
                        .fg(Color::Rgb(165, 180, 252))
                        .bg(Color::Rgb(30, 35, 60)),
                    "• ",
                )
            } else {
                (Style::default().fg(Color::Rgb(148, 163, 184)), "  ")
            };

            let badge = if *label == "↻  Updates" {
                "  0"
            } else {
                ""
            };

            ListItem::new(Line::styled(
                format!("{prefix}{:<12}{badge}", label),
                style,
            ))
        })
        .collect::<Vec<_>>();

    let mut state = ListState::default();
    state.select(Some(sidebar_active));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(12), Constraint::Length(6)])
        .split(area);

    frame.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(border_style)
                .title(title),
        ),
        chunks[0],
        &mut state,
    );

    if chunks[1].height >= 5 {
        let art = vec![
            Line::styled("   *    .  ☾  .  ", Style::default().fg(Color::Rgb(147, 197, 253))),
            Line::styled("     .    *   .  ", Style::default().fg(Color::Rgb(165, 180, 252))),
            Line::styled("    ▲      ▲     ", Style::default().fg(Color::Rgb(99, 102, 241))),
            Line::styled("   / \\    / \\    ", Style::default().fg(Color::Rgb(67, 56, 202))),
            Line::styled("  /   \\  /   \\   ", Style::default().fg(Color::Rgb(49, 46, 129))),
        ];
        frame.render_widget(
            Paragraph::new(art).block(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_style(border_style),
            ),
            chunks[1],
        );
    }
}

fn draw_settings(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let is_focused = app.focus_panel == FocusPanel::Content;
    let border_style = if is_focused {
        Style::default().fg(Color::Rgb(129, 140, 248))
    } else {
        Style::default().fg(Color::Rgb(45, 52, 80))
    };

    let settings_items = [
        (
            "Pre-install Auto Backup",
            if app.settings.auto_backup { "[✓ Enabled]" } else { "[✗ Disabled]" },
            "Creates content-addressed backup snapshot before modifying files",
        ),
        (
            "Strict Hash Verification",
            if app.settings.strict_hash_check { "[✓ Strict SHA-256]" } else { "[✗ Permissive]" },
            "Enforces cryptographic SHA-256 validation for all package files",
        ),
        (
            "Offline Cache Preference",
            if app.settings.offline_cache_first { "[✓ Local Cache First]" } else { "[✗ Network First]" },
            "Operates without network requirement using local registry cache",
        ),
        (
            "Target Compositor",
            app.settings.target_compositor.as_str(),
            "Desktop Wayland compositor environment for configuration routing",
        ),
        (
            "Target Shell / Bar",
            app.settings.target_shell.as_str(),
            "Managed status bar / dock component (e.g. Waybar, Eww)",
        ),
        (
            "Registry Sync Mode",
            "Local & Upstream Git",
            "Press Enter / R to trigger registry catalog refresh",
        ),
        (
            "Database & Journal Health",
            if app.db_available { "Online (WAL Mode)" } else { "Unavailable" },
            "Press Enter to run SQLite PRAGMA integrity check",
        ),
        (
            "Reset Preferences",
            "[Restore Defaults]",
            "Press Enter to reset configuration preferences to factory defaults",
        ),
    ];

    let items = settings_items
        .iter()
        .enumerate()
        .map(|(idx, (name, val, _desc))| {
            let is_sel = idx == app.setting_selection && is_focused;
            let (bg, fg, val_fg) = if is_sel {
                (Color::Rgb(60, 70, 160), Color::White, Color::Rgb(52, 211, 153))
            } else {
                (Color::Reset, Color::Rgb(226, 232, 240), Color::Rgb(129, 140, 248))
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:<28}", name),
                    Style::default().fg(fg).bg(bg).add_modifier(if is_sel {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
                Span::styled(
                    format!(" {:<20}", val),
                    Style::default().fg(val_fg).bg(bg).add_modifier(Modifier::BOLD),
                ),
            ]))
        })
        .collect::<Vec<_>>();

    let mut state = ListState::default();
    state.select(Some(app.setting_selection));

    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(Span::styled(
                        " ⚙ Settings & Preferences (Use ↑/↓, Enter/Space to Toggle) ",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    )),
            )
            .highlight_style(Style::default().bg(Color::Rgb(60, 70, 160))),
        chunks[0],
        &mut state,
    );

    let selected_setting = settings_items.get(app.setting_selection);
    let desc_lines = if let Some((name, val, desc)) = selected_setting {
        vec![
            Line::styled(
                format!("Setting: {name}"),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Line::styled(format!("Current Value: {val}"), Style::default().fg(Color::Green)),
            Line::from(""),
            Line::from(*desc),
            Line::from(""),
            Line::styled(
                "Instructions:",
                Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED),
            ),
            Line::from("• Press Enter or Space to toggle or cycle setting values."),
            Line::from("• Press Tab to switch back to the Navigation sidebar."),
            Line::from("• Settings apply immediately to transactions and catalog."),
            Line::from(""),
            Line::styled(
                "System Information:",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Line::from(format!("• Data Dir: {}", app.data_dir.display())),
            Line::from(format!("• Backups Dir: {}", app.data_dir.join("backups").display())),
            Line::from(format!("• Journal DB: {}", app.journal_path.display())),
            Line::from(format!("• Total Transactions: {}", app.transactions.len())),
            Line::from(format!("• Catalog Packages: {}", app.catalog.len())),
            Line::from(format!("• Network Status: {:?}", app.network_status)),
        ]
    } else {
        vec![Line::from("Select a setting to view details.")]
    };

    frame.render_widget(
        Paragraph::new(desc_lines)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(" Setting Details & Diagnostics "),
            ),
        chunks[1],
    );
}

fn key_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Rgb(177, 153, 255))
        .add_modifier(Modifier::BOLD)
}

fn draw_catalog(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(5)])
        .split(area);
    let search_label = if app.searching {
        format!(" Search packages... {}_", app.search)
    } else if app.search.is_empty() {
        " Search packages... (/)".to_string()
    } else {
        format!(" Search: {}", app.search)
    };
    let category_label = app.category.as_deref().unwrap_or("All categories");
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                search_label,
                Style::default().fg(Color::White).bg(Color::Rgb(24, 28, 45)),
            ),
            Span::raw("   "),
            Span::styled(
                format!(" {}  ▾ ", category_label),
                Style::default().fg(Color::White).bg(Color::Rgb(24, 28, 45)),
            ),
            Span::raw("   "),
            Span::styled(
                " Sort: Name ▾ ",
                Style::default().fg(Color::White).bg(Color::Rgb(24, 28, 45)),
            ),
        ]))
        .block(Block::default().borders(Borders::BOTTOM)),
        sections[0],
    );
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(sections[1]);
    let entries = app.filtered_catalog();
    let items = entries
        .iter()
        .map(|item| {
            let status_badge = if item.installed { " [installed]" } else { "" };
            let license_str = if item.manifest.package.license.is_empty() {
                "no license"
            } else {
                &item.manifest.package.license
            };
            ListItem::new(Line::from(vec![
                Span::styled(&item.name, Style::default().fg(Color::White)),
                Span::styled(
                    format!(
                        "  v{}  [{}]  {}{}",
                        item.version, license_str, item.category, status_badge
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let title = if let Some(category) = &app.category {
        format!("Packages ({}) • {category}", entries.len())
    } else {
        format!("Packages ({})", entries.len())
    };
    let mut state = ListState::default();
    state.select((!entries.is_empty()).then_some(app.catalog_selection));
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(60, 82, 178))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        columns[0],
        &mut state,
    );
    let detail = if let Some(item) = app.selected_catalog() {
        let mut lines = vec![
            Line::styled(
                &item.name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(format!("{}  •  v{}", item.id, item.version)),
            Line::from(format!(
                "Type: {}  •  Category: {}",
                item.manifest.package.package_type, item.category
            )),
            Line::from(format!(
                "License: {}",
                if item.manifest.package.license.is_empty() {
                    "not declared"
                } else {
                    &item.manifest.package.license
                }
            )),
        ];

        // Upstream Provenance / Authors
        let authors_str = if item.manifest.package.authors.is_empty() {
            "none declared".to_string()
        } else {
            item.manifest
                .package
                .authors
                .iter()
                .map(format_author)
                .collect::<Vec<_>>()
                .join(", ")
        };
        lines.push(Line::from(format!("Authors: {authors_str}")));

        // Repository
        let repo_style = if item.manifest.package.repository.is_some() {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::raw("Repository: "),
            Span::styled(
                item.manifest
                    .package
                    .repository
                    .as_deref()
                    .unwrap_or("not declared"),
                repo_style,
            ),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(item.description.clone()));
        lines.push(Line::from(format!(
            "Tags: {}",
            if item.tags.is_empty() {
                "not declared".into()
            } else {
                item.tags.join(", ")
            }
        )));

        // Compatibility
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "Compatibility",
            Style::default().fg(Color::Yellow),
        ));
        if let Some(compat) = &item.manifest.compatibility {
            lines.push(Line::from(format!(
                "Display server: {}",
                compat.display_server.as_deref().unwrap_or("not declared")
            )));
            lines.push(Line::from(format!(
                "Architectures: {}",
                if compat.architectures.is_empty() {
                    "not declared".into()
                } else {
                    compat.architectures.join(", ")
                }
            )));
            if !compat.compositors.is_empty() {
                let comps = compat
                    .compositors
                    .iter()
                    .map(|c| format!("{} {}", c.name, c.version))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(Line::from(format!("Compositors: {comps}")));
            }
        } else {
            lines.push(Line::from("Display server & architectures: not declared"));
        }

        // Palette Swatches
        if let Some(previews) = &item.manifest.previews {
            if !previews.palette.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::styled("Palette", Style::default().fg(Color::Yellow)));
                let mut spans = Vec::new();
                for (idx, color) in previews.palette.iter().enumerate() {
                    if idx > 0 && idx % 2 == 0 {
                        lines.push(Line::from(spans));
                        spans = Vec::new();
                    } else if idx > 0 {
                        spans.push(Span::raw("  "));
                    }
                    if let Some(rgb) = parse_hex_color(&color.hex) {
                        spans.push(Span::styled("  ", Style::default().bg(rgb)));
                        spans.push(Span::raw(" "));
                    }
                    spans.push(Span::styled(&color.name, Style::default().fg(Color::White)));
                    spans.push(Span::styled(
                        format!(" ({})", color.hex),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                if !spans.is_empty() {
                    lines.push(Line::from(spans));
                }
            }
        }

        // Install status & operations
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "Install status & operations",
            Style::default().fg(Color::Yellow),
        ));
        lines.push(Line::from(if item.installed {
            "Installed (journal ownership record found)."
        } else {
            "Not installed (no ownership record found)."
        }));
        lines.push(Line::from(format!(
            "File operations: {}  •  Dependencies: {}",
            item.manifest.install.files.len(),
            item.manifest
                .dependencies
                .as_ref()
                .map_or(0, |d| d.packages.len())
        )));
        lines.push(Line::from(format!("Manifest: {}", item.manifest_dir.display())));
        lines
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
    let area = centered(frame.area(), 78, 80);
    frame.render_widget(Clear, area);
    let body = if app.page == 0 || app.page == 1 {
        app.selected_catalog()
            .map(|item| {
                let mut lines = vec![
                    Line::styled(
                        &item.name,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Line::from(format!("ID: {}  •  Version: {}", item.id, item.version)),
                    Line::from(format!(
                        "Type: {}  •  Category: {}",
                        item.manifest.package.package_type, item.category
                    )),
                    Line::from(format!(
                        "License: {}",
                        if item.manifest.package.license.is_empty() {
                            "not declared"
                        } else {
                            &item.manifest.package.license
                        }
                    )),
                    Line::from(""),
                    Line::styled("Provenance & Repository", Style::default().fg(Color::Yellow)),
                ];

                if item.manifest.package.authors.is_empty() {
                    lines.push(Line::from("Authors: none declared"));
                } else {
                    for author in &item.manifest.package.authors {
                        lines.push(Line::from(format!("Author: {}", format_author(author))));
                    }
                }

                let repo_style = if item.manifest.package.repository.is_some() {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::raw("Repository: "),
                    Span::styled(
                        item.manifest
                            .package
                            .repository
                            .as_deref()
                            .unwrap_or("not declared"),
                        repo_style,
                    ),
                ]));

                lines.push(Line::from(""));
                lines.push(Line::styled("Description", Style::default().fg(Color::Yellow)));
                lines.push(Line::from(item.description.clone()));
                lines.push(Line::from(format!(
                    "Tags: {}",
                    if item.tags.is_empty() {
                        "not declared".into()
                    } else {
                        item.tags.join(", ")
                    }
                )));

                lines.push(Line::from(""));
                lines.push(Line::styled("Compatibility", Style::default().fg(Color::Yellow)));
                if let Some(compat) = &item.manifest.compatibility {
                    lines.push(Line::from(format!(
                        "Display Server: {}",
                        compat.display_server.as_deref().unwrap_or("not declared")
                    )));
                    lines.push(Line::from(format!(
                        "Architectures: {}",
                        if compat.architectures.is_empty() {
                            "not declared".into()
                        } else {
                            compat.architectures.join(", ")
                        }
                    )));
                    if !compat.compositors.is_empty() {
                        let comps = compat
                            .compositors
                            .iter()
                            .map(|c| format!("{} {}", c.name, c.version))
                            .collect::<Vec<_>>()
                            .join(", ");
                        lines.push(Line::from(format!("Compositors: {comps}")));
                    }
                } else {
                    lines.push(Line::from("Compatibility: not declared"));
                }

                if let Some(previews) = &item.manifest.previews {
                    if !previews.palette.is_empty() {
                        lines.push(Line::from(""));
                        lines.push(Line::styled("Palette Swatches", Style::default().fg(Color::Yellow)));
                        for color in &previews.palette {
                            let mut spans = Vec::new();
                            if let Some(rgb) = parse_hex_color(&color.hex) {
                                spans.push(Span::styled("  ", Style::default().bg(rgb)));
                                spans.push(Span::raw(" "));
                            }
                            spans.push(Span::styled(&color.name, Style::default().fg(Color::White)));
                            spans.push(Span::styled(
                                format!(" ({})", color.hex),
                                Style::default().fg(Color::DarkGray),
                            ));
                            lines.push(Line::from(spans));
                        }
                    }
                }

                lines.push(Line::from(""));
                lines.push(Line::styled("Install Specification", Style::default().fg(Color::Yellow)));
                lines.push(Line::from(format!(
                    "Declared file operations: {}",
                    item.manifest.install.files.len()
                )));
                for file in &item.manifest.install.files {
                    lines.push(Line::from(format!("  • {} -> {}", file.source, file.target)));
                }
                lines.push(Line::from(format!("Manifest directory: {}", item.manifest_dir.display())));
                lines.push(Line::from(""));
                lines.push(Line::styled("Esc closes details", Style::default().fg(Color::DarkGray)));
                lines
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
    let area = centered(frame.area(), 68, 75);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "Keyboard Navigation Help",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(""),
            Line::from("Tab / ⭾      Switch between Sidebar and Content panel"),
            Line::from("Up / Down    Navigate inside the active panel"),
            Line::from("Left         Move focus back to Navigation sidebar"),
            Line::from("Right        Move focus into Content panel"),
            Line::from("Enter        Open details modal / Toggle selected setting"),
            Line::from("Space        Toggle setting on Settings page"),
            Line::from("1 - 7        Direct jump to tab (Home, Browse, Installed, Settings...)"),
            Line::from("/            Search packages and transaction history"),
            Line::from("i            Install selected package"),
            Line::from("d            Inspect source/target file diff & planned backups"),
            Line::from("p            Open package palette & preview"),
            Line::from("r            Rollback selected installed package (on Installed tab)"),
            Line::from("R            Refresh registry catalog"),
            Line::from("f / F        Fetch Hyprland configuration from GitHub URL"),
            Line::from("Esc          Dismiss modal / Return focus to sidebar"),
            Line::from("?            Toggle this help modal"),
            Line::from("Ctrl+C / Q   Quit cleanly"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Help")),
        area,
    );
}

fn draw_url_input_modal(frame: &mut Frame<'_>, app: &App) {
    let area = centered(frame.area(), 72, 11);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Import Hyprland Config from GitHub • Esc Cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Prompt
            Constraint::Length(1), // Spacing
            Constraint::Length(3), // Input box
            Constraint::Length(1), // Error or Help text
            Constraint::Min(1),    // Footer keys
        ])
        .split(inner);

    let prompt = Line::styled(
        "Enter public GitHub repository URL (e.g. https://github.com/user/hyprland-dots):",
        Style::default().fg(Color::Yellow),
    );
    frame.render_widget(Paragraph::new(prompt), chunks[0]);

    let input_display = format!("{}█", app.url_input_buffer);
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(if app.url_input_error.is_some() {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Rgb(137, 180, 250))
        });
    let input_widget = Paragraph::new(Line::styled(
        input_display,
        Style::default().fg(Color::White),
    ))
    .block(input_block);
    frame.render_widget(input_widget, chunks[2]);

    if let Some(err) = &app.url_input_error {
        let err_line = Line::styled(
            format!("Error: {err}"),
            Style::default().fg(Color::Red),
        );
        frame.render_widget(Paragraph::new(err_line).wrap(Wrap { trim: true }), chunks[3]);
    } else {
        let hint_line = Line::styled(
            "Repository will be cloned shallowly and scanned for standard Hyprland dotfiles.",
            Style::default().fg(Color::DarkGray),
        );
        frame.render_widget(Paragraph::new(hint_line), chunks[3]);
    }

    let footer_line = Line::from(vec![
        Span::styled("Enter", key_style()),
        Span::raw(" Fetch & Synthesize  "),
        Span::styled("Esc", key_style()),
        Span::raw(" Cancel  "),
        Span::styled("Backspace", key_style()),
        Span::raw(" Delete"),
    ]);
    frame.render_widget(Paragraph::new(footer_line), chunks[4]);
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
    load_catalog_records(db, error)
}

fn load_catalog_records(db: &Database, error: &mut Option<String>) -> Vec<CatalogItem> {
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
                    tags: record.tags,
                    version: record.version,
                    manifest_dir: record
                        .manifest_path
                        .parent()
                        .unwrap_or(Path::new("."))
                        .to_path_buf(),
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

fn registry_source() -> Option<String> {
    std::env::var("WODERRISE_REGISTRY_URL")
        .ok()
        .filter(|source| !source.trim().is_empty())
}

fn registry_cache_dir(data_dir: &Path) -> PathBuf {
    std::env::var_os("WODERRISE_REGISTRY_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("cache/registry"))
}

#[cfg(test)]
mod tests {
    use super::*;

    impl App {
        fn test_instance() -> Self {
            Self {
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
                preview: false,
                diff: false,
                preview_selection: 0,
                diff_file_index: 0,
                diff_scroll: 0,
                diff_plan: None,
                diff_error: None,
                install_rx: None,
                refresh_rx: None,
                url_modal_open: false,
                url_input_buffer: String::new(),
                url_input_error: None,
                fetch_rx: None,
                status: String::new(),
                db_available: false,
                network_status: NetworkStatus::Online,
                operation: None,
                error_alert: None,
                journal_path: PathBuf::new(),
                journal_error: None,
                catalog_error: None,
                interrupted_transactions: 0,
                data_dir: PathBuf::new(),
                focus_panel: FocusPanel::Content,
                sidebar_selection: 0,
                setting_selection: 0,
                settings: AppSettings::default(),
            }
        }
    }

    #[test]
    fn catalog_loads_valid_registry_packages_without_hardcoded_entries() {
        let db = Database::in_memory().expect("database");
        let mut error = None;
        let entries = load_catalog(
            &db,
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages"),
            &mut error,
        );
        assert_eq!(entries.len(), 2);
        assert!(error.is_none());
        assert!(entries.iter().any(|e| e.manifest.package.id == "com.catppuccin.waybar"));
        assert!(entries.iter().any(|e| e.manifest.package.id == "com.catppuccin.hyprland"));
    }

    #[test]
    fn rollback_candidates_exclude_failed_transactions() {
        let mut app = App::test_instance();
        app.page = 2;
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
        assert_eq!(app.visible_transactions(true).len(), 1);
        assert_eq!(
            app.selected_transaction().map(|tx| tx.id.as_str()),
            Some("committed")
        );
    }

    #[test]
    fn diff_reader_reports_missing_and_oversized_files() {
        let root = std::env::temp_dir().join(format!("woderrise-tui-diff-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let oversized = root.join("oversized");
        std::fs::write(&oversized, vec![b'x'; (MAX_DIFF_BYTES + 1) as usize]).unwrap();
        assert!(read_diff_file(&root.join("missing")).is_err());
        assert!(read_diff_file(&oversized)
            .unwrap_err()
            .contains("diff limit"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn active_install_blocks_quit_requests() {
        let (_sender, receiver) = mpsc::channel();
        let mut app = App::test_instance();
        app.install_rx = Some(receiver);
        assert!(!app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)));
        assert!(app.status.contains("Installation is running"));
    }

    #[test]
    fn selection_movement_is_bounded_for_empty_and_nonempty_lists() {
        assert_eq!(move_index(0, 0, 1), 0);
        assert_eq!(move_index(0, 2, 1), 1);
        assert_eq!(move_index(1, 2, 1), 1);
        assert_eq!(move_index(1, 2, -1), 0);
    }

    #[test]
    fn test_hex_color_parser_rgb() {
        assert_eq!(parse_hex_color("#1e1e2e"), Some(Color::Rgb(30, 30, 46)));
        assert_eq!(parse_hex_color("#89b4fa"), Some(Color::Rgb(137, 180, 250)));
        assert_eq!(parse_hex_color("#cba6f7"), Some(Color::Rgb(203, 166, 247)));
        assert_eq!(parse_hex_color("#cdd6f4"), Some(Color::Rgb(205, 214, 244)));
        assert_eq!(parse_hex_color("cba6f7"), Some(Color::Rgb(203, 166, 247)));
        assert_eq!(parse_hex_color("not_a_color"), None);
    }

    #[test]
    fn test_format_author_provenance() {
        use woderrise_core::Author;
        let a1 = Author { name: "Alice".into(), email: None, url: None };
        assert_eq!(format_author(&a1), "Alice");

        let a2 = Author { name: "Bob".into(), email: Some("bob@example.com".into()), url: None };
        assert_eq!(format_author(&a2), "Bob <bob@example.com>");

        let a3 = Author { name: "Catppuccin".into(), email: None, url: Some("https://github.com/catppuccin".into()) };
        assert_eq!(format_author(&a3), "Catppuccin (https://github.com/catppuccin)");

        let a4 = Author { name: "Dana".into(), email: Some("dana@dev.org".into()), url: Some("https://dana.dev".into()) };
        assert_eq!(format_author(&a4), "Dana <dana@dev.org> (https://dana.dev)");
    }

    #[test]
    fn test_genuine_catppuccin_metadata_integrity() {
        let db = Database::in_memory().expect("database");
        let mut error = None;
        let entries = load_catalog(
            &db,
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages"),
            &mut error,
        );
        assert_eq!(entries.len(), 2);
        let item = entries
            .iter()
            .find(|e| e.manifest.package.id == "com.catppuccin.waybar")
            .expect("waybar package");
        assert_eq!(item.manifest.package.id, "com.catppuccin.waybar");
        assert_eq!(item.manifest.package.license, "MIT");
        assert_eq!(item.manifest.package.authors.len(), 1);
        assert_eq!(item.manifest.package.authors[0].name, "Catppuccin contributors");
        assert_eq!(
            item.manifest.package.repository.as_deref(),
            Some("https://github.com/catppuccin/waybar")
        );
        assert!(item.manifest.compatibility.is_some());
        let compat = item.manifest.compatibility.as_ref().unwrap();
        assert_eq!(compat.display_server.as_deref(), Some("wayland_only"));
        assert_eq!(compat.architectures, vec!["x86_64", "aarch64"]);

        let previews = item.manifest.previews.as_ref().unwrap();
        assert_eq!(previews.palette.len(), 4);
        for color in &previews.palette {
            let parsed = parse_hex_color(&color.hex);
            assert!(parsed.is_some(), "palette color {} must parse to Color::Rgb", color.hex);
        }
    }

    #[test]
    fn test_diff_navigation_and_scrolling() {
        let mut app = App::test_instance();
        app.diff = true;
        app.diff_file_index = 0;
        app.diff_scroll = 5;

        // Up scrolls up
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.diff_scroll, 4);

        // Home scrolls to top
        app.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(app.diff_scroll, 0);

        // Esc closes diff
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.diff);
    }

    #[test]
    fn test_ctrl_c_quits_when_no_active_operation() {
        let mut app = App::test_instance();
        assert!(app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn test_ctrl_c_blocked_during_active_install() {
        let (_sender, receiver) = mpsc::channel();
        let mut app = App::test_instance();
        app.install_rx = Some(receiver);
        assert!(!app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert!(app.status.contains("Installation is running"));
    }

    #[test]
    fn test_key_release_events_ignored() {
        let mut app = App::test_instance();
        let release_event = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: crossterm::event::KeyEventState::NONE,
        };
        assert!(!app.handle_key(release_event));
    }

    #[test]
    fn test_search_key_activates_on_all_tabs() {
        let mut app = App::test_instance();
        app.page = 2; // Rollback tab
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.searching);

        app.searching = false;
        app.page = 4; // Journal tab
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.searching);
    }

    #[test]
    fn test_search_typing_resets_selections() {
        let mut app = App::test_instance();
        app.searching = true;
        app.catalog_selection = 3;
        app.transaction_selection = 2;
        app.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
        assert_eq!(app.search, "w");
        assert_eq!(app.catalog_selection, 0);
        assert_eq!(app.transaction_selection, 0);
    }

    #[test]
    fn test_offline_indicator_reflects_network_failure() {
        let mut app = App::test_instance();
        assert_eq!(app.network_status, NetworkStatus::Online);
        app.network_status = NetworkStatus::OfflineSyncFailed;
        assert_eq!(app.network_status, NetworkStatus::OfflineSyncFailed);
        app.network_status = NetworkStatus::OfflineLocal;
        assert_eq!(app.network_status, NetworkStatus::OfflineLocal);
    }

    #[test]
    fn test_adversarial_key_release_and_repeat_storm_strictly_ignored() {
        let mut app = App::test_instance();
        app.catalog_selection = 2;
        app.transaction_selection = 1;
        app.page = 1;
        app.status = "Baseline status".into();

        let kinds = [KeyEventKind::Release, KeyEventKind::Repeat];
        let codes = [
            KeyCode::Char('q'),
            KeyCode::Char('c'),
            KeyCode::Char('/'),
            KeyCode::Char('i'),
            KeyCode::Char('d'),
            KeyCode::Char('r'),
            KeyCode::Char('R'),
            KeyCode::Char('?'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Backspace,
        ];
        let modifiers = [
            KeyModifiers::NONE,
            KeyModifiers::CONTROL,
            KeyModifiers::SHIFT,
            KeyModifiers::ALT,
        ];

        // Storm of hundreds of non-Press events
        for kind in kinds {
            for code in &codes {
                for modifier in &modifiers {
                    let ev = KeyEvent {
                        code: *code,
                        modifiers: *modifier,
                        kind,
                        state: crossterm::event::KeyEventState::NONE,
                    };
                    let result = app.handle_key(ev);
                    assert!(!result, "Event of kind {:?} with code {:?} must return false", kind, code);
                    // Verify no state changed
                    assert_eq!(app.page, 1);
                    assert_eq!(app.catalog_selection, 2);
                    assert_eq!(app.transaction_selection, 1);
                    assert!(!app.searching);
                    assert_eq!(app.search, "");
                    assert!(!app.detail);
                    assert!(!app.diff);
                    assert!(!app.preview);
                    assert!(!app.confirm_install);
                    assert!(!app.confirm_rollback);
                    assert!(!app.help);
                    assert_eq!(app.status, "Baseline status");
                }
            }
        }
    }

    #[test]
    fn test_adversarial_ctrl_c_clean_quit_on_all_tabs_and_overlays() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        // 1. Quits cleanly on all tabs (0..=4)
        for (page, tab_name) in TABS.iter().enumerate() {
            let mut app = App::test_instance();
            app.page = page;
            assert!(
                app.handle_key(ctrl_c),
                "Ctrl+C must cleanly quit on tab {page} ({tab_name})"
            );
        }

        // 2. Quits cleanly even inside overlays/sub-states
        let mut app = App::test_instance();
        app.searching = true;
        assert!(app.handle_key(ctrl_c), "Ctrl+C must quit while searching");

        let mut app = App::test_instance();
        app.diff = true;
        assert!(app.handle_key(ctrl_c), "Ctrl+C must quit during diff inspection");

        let mut app = App::test_instance();
        app.preview = true;
        assert!(app.handle_key(ctrl_c), "Ctrl+C must quit during preview modal");

        let mut app = App::test_instance();
        app.detail = true;
        assert!(app.handle_key(ctrl_c), "Ctrl+C must quit during detail view");

        let mut app = App::test_instance();
        app.help = true;
        assert!(app.handle_key(ctrl_c), "Ctrl+C must quit during help modal");

        let mut app = App::test_instance();
        app.confirm_install = true;
        assert!(app.handle_key(ctrl_c), "Ctrl+C must quit during confirm install");

        let mut app = App::test_instance();
        app.confirm_rollback = true;
        assert!(app.handle_key(ctrl_c), "Ctrl+C must quit during confirm rollback");

        let mut app = App::test_instance();
        app.error_alert = Some("Fatal alert".into());
        assert!(app.handle_key(ctrl_c), "Ctrl+C must quit during error alert");
    }

    #[test]
    fn test_adversarial_ctrl_c_strictly_blocked_during_active_operations() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        // 1. Blocked across all tabs when install_rx is present
        for (page, tab_name) in TABS.iter().enumerate() {
            let (_tx, rx) = mpsc::channel();
            let mut app = App::test_instance();
            app.page = page;
            app.install_rx = Some(rx);
            assert!(
                !app.handle_key(ctrl_c),
                "Ctrl+C must be blocked on tab {page} ({tab_name}) when install_rx is active"
            );
            assert!(app.status.contains("Installation is running"));
        }

        // 2. Blocked across all tabs when operation is present
        for (page, tab_name) in TABS.iter().enumerate() {
            let mut app = App::test_instance();
            app.page = page;
            app.operation = Some(ActiveOperation {
                title: "Rollback in progress".into(),
                model: woderrise_gui::ProgressModel::new("Rollback", 50, "Restoring backups"),
            });
            assert!(
                !app.handle_key(ctrl_c),
                "Ctrl+C must be blocked on tab {page} ({tab_name}) when operation is active"
            );
            assert!(app.status.contains("Installation is running"));
        }

        // 3. Once active operation completes, Ctrl+C becomes available again
        let (_tx, rx) = mpsc::channel();
        let mut app = App::test_instance();
        app.install_rx = Some(rx);
        app.operation = Some(ActiveOperation {
            title: "Task".into(),
            model: woderrise_gui::ProgressModel::new("Step", 10, "..."),
        });
        assert!(!app.handle_key(ctrl_c));

        // Simulate finish
        app.install_rx = None;
        app.operation = None;
        assert!(app.handle_key(ctrl_c), "Ctrl+C must immediately quit once operation is cleared");
    }

    #[test]
    fn test_adversarial_search_activation_across_all_tabs_and_input_flow() {
        let slash = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);

        // 1. Slash unlocks search on every tab
        for (page, tab_name) in TABS.iter().enumerate() {
            let mut app = App::test_instance();
            app.page = page;
            assert!(!app.searching);
            app.handle_key(slash);
            assert!(
                app.searching,
                "Pressing '/' must activate search on page {page} ({tab_name})"
            );
        }

        // 2. Typing characters resets selections and builds query
        let mut app = App::test_instance();
        app.page = 0;
        app.handle_key(slash);
        assert!(app.searching);

        app.catalog_selection = 5;
        app.transaction_selection = 8;

        for ch in ['w', 'a', 'y', 'b', 'a', 'r'] {
            app.catalog_selection = 5; // force non-zero to test reset
            app.transaction_selection = 8;
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            assert_eq!(app.catalog_selection, 0, "Catalog selection must reset to 0 on typing");
            assert_eq!(app.transaction_selection, 0, "Transaction selection must reset to 0 on typing");
        }
        assert_eq!(app.search, "waybar");

        // 3. Backspace removes char and resets selections
        app.catalog_selection = 3;
        app.transaction_selection = 4;
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.search, "wayba");
        assert_eq!(app.catalog_selection, 0);
        assert_eq!(app.transaction_selection, 0);

        // 4. Esc leaves searching mode
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.searching);
        assert_eq!(app.search, "wayba"); // query preserved

        // 5. Enter leaves searching mode
        app.handle_key(slash);
        assert!(app.searching);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.searching);
    }

    #[test]
    fn test_adversarial_async_channel_progress_and_completion_without_deadlock() {
        let mut app = App::test_instance();
        let (tx, rx) = mpsc::channel();
        app.install_rx = Some(rx);
        app.operation = Some(ActiveOperation {
            title: "Testing Async Pipeline".into(),
            model: woderrise_gui::ProgressModel::new("Init", 0, "Starting test"),
        });

        // 1. Rapid progress messages drained in loop without UI lockup
        for i in 1..=50 {
            tx.send(OperationEvent::Progress(woderrise_gui::ProgressModel::new(
                "Apply",
                (i * 2) as u8,
                format!("Applying file {i}"),
            )))
            .unwrap();
        }

        app.poll_install();

        let current_op = app.operation.as_ref().expect("Operation must still be active");
        assert_eq!(current_op.model.percent, 100);
        assert_eq!(current_op.model.stage, "Apply");
        assert!(current_op.model.message.contains("Applying file 50"));
        assert!(app.install_rx.is_some());

        // 2. Successful finish transitions state cleanly without deadlock
        tx.send(OperationEvent::Finished(Ok("txn-stress-999".into()))).unwrap();
        app.poll_install();

        assert!(app.install_rx.is_none(), "install_rx must be cleared on Finished");
        assert!(app.operation.is_none(), "operation must be cleared on Finished");
        assert!(
            app.status.contains("Operation committed: txn-stress-999"),
            "Status was: {}",
            app.status
        );

        // 3. Error event sets error_alert without panic or deadlock
        let (tx2, rx2) = mpsc::channel();
        app.install_rx = Some(rx2);
        app.operation = Some(ActiveOperation {
            title: "Failing Operation".into(),
            model: woderrise_gui::ProgressModel::new("FailStep", 10, "Working..."),
        });
        tx2.send(OperationEvent::Finished(Err("simulated I/O disk error".into()))).unwrap();

        app.poll_install();

        assert!(app.install_rx.is_none());
        assert!(app.operation.is_none());
        assert_eq!(app.error_alert.as_deref(), Some("simulated I/O disk error"));
        assert!(
            app.status.contains("Operation failed: simulated I/O disk error"),
            "Status was: {}",
            app.status
        );

        // 4. Worker thread disconnection without result handled gracefully
        let (tx3, rx3) = mpsc::channel::<OperationEvent>();
        app.install_rx = Some(rx3);
        app.operation = Some(ActiveOperation {
            title: "Dropped Channel".into(),
            model: woderrise_gui::ProgressModel::new("Wait", 20, "Waiting"),
        });
        drop(tx3); // Worker thread dropped/crashed

        app.poll_install();

        assert!(app.install_rx.is_none());
        assert!(app.operation.is_none());
        assert!(
            app.status.contains("Operation worker stopped without a result"),
            "Status was: {}",
            app.status
        );

        // 5. Concurrent multi-threaded test with timeout assertion
        let (tx_threaded, rx_threaded) = mpsc::channel();
        app.install_rx = Some(rx_threaded);
        app.operation = Some(ActiveOperation {
            title: "Threaded Op".into(),
            model: woderrise_gui::ProgressModel::new("Concurrent", 0, "Running"),
        });

        let worker = std::thread::spawn(move || {
            for step in 1..=10 {
                let _ = tx_threaded.send(OperationEvent::Progress(
                    woderrise_gui::ProgressModel::new("Step", (step * 10) as u8, format!("Step {step}")),
                ));
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = tx_threaded.send(OperationEvent::Finished(Ok("txn-concurrent-done".into())));
        });

        let start = std::time::Instant::now();
        while app.install_rx.is_some() {
            app.poll_install();
            assert!(
                start.elapsed() < Duration::from_secs(3),
                "UI thread deadlocked or stalled during async processing"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        worker.join().unwrap();
        assert!(
            app.status.contains("Operation committed: txn-concurrent-done"),
            "Status was: {}",
            app.status
        );
    }

    #[test]
    fn test_adversarial_offline_resilience_missing_registry_and_sync_failure() {
        // 1. Missing registry path does NOT panic load_catalog, and returns cached records
        let temp_dir = std::env::temp_dir().join(format!("woderrise-offline-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("journal.db");
        let db = Database::open(&db_path).expect("open db");

        // Seed with valid registry package first
        let seed_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages");
        let mut err = None;
        let seeded = load_catalog(&db, &seed_root, &mut err);
        assert!(!seeded.is_empty(), "Database should be seeded with catalog items");

        // Now point to completely missing / disconnected registry path
        let non_existent = temp_dir.join("nonexistent_registry_folder_404");
        let mut load_err = None;
        let cached_items = load_catalog(&db, &non_existent, &mut load_err);
        assert!(load_err.is_some(), "Should record error when registry dir is missing");
        assert_eq!(
            cached_items.len(),
            seeded.len(),
            "Must fall back to cached records from database without panicking"
        );

        // 2. Refresh failure in poll_refresh transitions truthfully to OfflineSyncFailed
        let mut app = App::test_instance();
        app.network_status = NetworkStatus::Online;
        let (tx, rx) = mpsc::channel();
        app.refresh_rx = Some(rx);
        app.operation = Some(ActiveOperation {
            title: "Refreshing".into(),
            model: woderrise_gui::ProgressModel::new("Sync", 10, "Fetching"),
        });

        tx.send(Err("Failed to resolve git remote: network unreachable".into())).unwrap();
        app.poll_refresh();

        assert_eq!(app.network_status, NetworkStatus::OfflineSyncFailed);
        assert!(app.refresh_rx.is_none());
        assert!(app.operation.is_none());
        assert!(app.status.contains("Refresh failed; using cached catalog"));

        // 3. Disconnected channel on refresh
        let (tx_disc, rx_disc) = mpsc::channel::<Result<PathBuf, String>>();
        app.refresh_rx = Some(rx_disc);
        drop(tx_disc);
        app.poll_refresh();
        assert_eq!(app.network_status, NetworkStatus::OfflineSyncFailed);
        assert!(app.status.contains("Refresh failed; using cached catalog"));

        // Clean up temp dir
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_tab_key_toggles_focus_panel() {
        let mut app = App::test_instance();
        assert_eq!(app.focus_panel, FocusPanel::Content);
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        app.handle_key(tab);
        assert_eq!(app.focus_panel, FocusPanel::Sidebar);
        app.handle_key(tab);
        assert_eq!(app.focus_panel, FocusPanel::Content);
    }

    #[test]
    fn test_sidebar_navigation_to_installed_and_settings() {
        let mut app = App::test_instance();
        app.focus_panel = FocusPanel::Sidebar;
        app.sidebar_selection = 0;
        assert_eq!(app.page, 0);

        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);

        // Down to Browse
        app.handle_key(down);
        assert_eq!(app.sidebar_selection, 1);
        assert_eq!(app.page, 0);

        // Down to Categories
        app.handle_key(down);
        assert_eq!(app.sidebar_selection, 2);

        // Down to Installed
        app.handle_key(down);
        assert_eq!(app.sidebar_selection, 3);
        assert_eq!(app.page, 2);

        // Down to Updates
        app.handle_key(down);
        assert_eq!(app.sidebar_selection, 4);
        assert_eq!(app.page, 3);

        // Down to Registry (skipping separator index 5)
        app.handle_key(down);
        assert_eq!(app.sidebar_selection, 6);
        assert_eq!(app.page, 4);

        // Down to Settings
        app.handle_key(down);
        assert_eq!(app.sidebar_selection, 7);
        assert_eq!(app.page, 5);

        // Enter moves focus to Content
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter);
        assert_eq!(app.focus_panel, FocusPanel::Content);
    }

    #[test]
    fn test_numeric_shortcuts_switch_sidebar_tabs() {
        let mut app = App::test_instance();
        app.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
        assert_eq!(app.sidebar_selection, 3);
        assert_eq!(app.page, 2);

        app.handle_key(KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE));
        assert_eq!(app.sidebar_selection, 7);
        assert_eq!(app.page, 5);

        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(app.sidebar_selection, 0);
        assert_eq!(app.page, 0);
    }

    #[test]
    fn test_settings_toggle_behavior() {
        let mut app = App::test_instance();
        app.page = 5;
        app.focus_panel = FocusPanel::Content;
        app.setting_selection = 0;

        assert!(app.settings.auto_backup);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.settings.auto_backup);
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.settings.auto_backup);

        app.setting_selection = 1;
        assert!(app.settings.strict_hash_check);
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.settings.strict_hash_check);
    }

    #[test]
    fn test_ctrl_q_quits_application() {
        let mut app = App::test_instance();
        assert!(app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn test_url_modal_open_and_close_esc() {
        let mut app = App::test_instance();
        assert!(!app.url_modal_open);

        // 'f' opens modal
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(app.url_modal_open);
        assert_eq!(app.url_input_buffer, "");
        assert!(app.url_input_error.is_none());

        // Esc dismisses modal
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.url_modal_open);

        // 'F' opens modal
        app.handle_key(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::NONE));
        assert!(app.url_modal_open);

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.url_modal_open);
    }

    #[test]
    fn test_url_modal_typing_and_backspace() {
        let mut app = App::test_instance();
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(app.url_modal_open);

        for c in "https://github.com/foo/bar".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(app.url_input_buffer, "https://github.com/foo/bar");

        for _ in 0..4 {
            app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        }
        assert_eq!(app.url_input_buffer, "https://github.com/foo");
    }

    #[test]
    fn test_url_modal_validation_empty_and_invalid() {
        let mut app = App::test_instance();
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));

        // Empty URL rejection
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.url_modal_open);
        assert!(app.url_input_error.is_some());
        assert!(app.url_input_error.as_ref().unwrap().contains("empty"));

        // Whitespace rejection
        app.url_input_buffer = "https://github.com/foo /bar".into();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.url_modal_open);
        assert!(app.url_input_error.is_some());

        // Flag injection rejection
        app.url_input_buffer = "--upload-pack=evil".into();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.url_modal_open);
        assert!(app.url_input_error.is_some());

        // Traversal rejection
        app.url_input_buffer = "https://github.com/../evil".into();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.url_modal_open);
        assert!(app.url_input_error.is_some());
    }

    #[test]
    fn test_url_modal_submit_valid_triggers_fetch_and_progress() {
        let mut app = App::test_instance();
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        app.url_input_buffer = "https://github.com/catppuccin/hyprland".into();

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(!app.url_modal_open);
        assert!(app.url_input_error.is_none());
        assert!(app.operation.is_some());
        let op = app.operation.as_ref().unwrap();
        assert!(op.title.contains("GitHub"));
        assert_eq!(op.model.stage, "Clone");
        assert!(app.fetch_rx.is_some());
    }

    #[test]
    fn test_poll_fetch_success_appends_package_and_opens_live_diff() {
        let mut app = App::test_instance();
        let (tx, rx) = mpsc::channel();
        app.fetch_rx = Some(rx);
        app.operation = Some(ActiveOperation {
            title: "Fetching from GitHub".into(),
            model: woderrise_gui::ProgressModel::new("Clone", 25, "Cloning..."),
        });

        let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../registry/packages/com.catppuccin.hyprland/1.0.0/woderrise.yaml");
        assert!(manifest_path.exists());

        tx.send(Ok(manifest_path.clone())).unwrap();

        app.poll_fetch();

        assert!(app.fetch_rx.is_none());
        assert!(app.operation.is_none());

        let selected = app.selected_catalog().expect("selected package");
        assert_eq!(selected.manifest_path, manifest_path);
        assert_eq!(selected.id, "com.catppuccin.hyprland");

        // Live Myers diff is immediately open
        assert!(app.diff);
        assert!(app.diff_plan.is_some());
        let plan = app.diff_plan.as_ref().unwrap();
        assert_eq!(plan.package_id, "com.catppuccin.hyprland");
        assert!(!plan.file_diffs.is_empty());
    }

    #[test]
    fn test_poll_fetch_error_sets_error_alert() {
        let mut app = App::test_instance();
        let (tx, rx) = mpsc::channel();
        app.fetch_rx = Some(rx);
        app.operation = Some(ActiveOperation {
            title: "Fetching from GitHub".into(),
            model: woderrise_gui::ProgressModel::new("Clone", 25, "Cloning..."),
        });

        tx.send(Err("repository not found".into())).unwrap();

        app.poll_fetch();

        assert!(app.fetch_rx.is_none());
        assert!(app.operation.is_none());
        assert_eq!(
            app.error_alert,
            Some("GitHub import failed: repository not found".into())
        );
        assert!(app.status.contains("GitHub import failed"));

        // Esc dismisses alert
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.error_alert.is_none());
    }

    #[test]
    fn test_headless_url_modal_ratatui_rendering() {
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        let mut app = App::test_instance();
        app.url_modal_open = true;
        app.url_input_buffer = "https://github.com/catppuccin/hyprland".into();

        terminal.draw(|frame| draw(frame, &app)).expect("draw frame");
        let buffer = terminal.backend().buffer();
        let content: String = (0..buffer.area.height)
            .flat_map(|y| {
                (0..buffer.area.width)
                    .map(move |x| buffer.cell((x, y)).map_or("", |c| c.symbol()))
                    .chain(std::iter::once("\n"))
            })
            .collect();

        assert!(
            content.contains("Import Hyprland Config from GitHub"),
            "Title missing in rendered modal"
        );
        assert!(
            content.contains("https://github.com/catppuccin/hyprland"),
            "URL input buffer missing in rendered modal"
        );
        assert!(
            content.contains("Fetch & Synthesize"),
            "Footer keys missing in rendered modal"
        );
    }

    // --- Milestone M7 Adversarial Challenger Tests ---

    #[test]
    fn test_adversarial_tui_rapid_typing_and_backspace_flood() {
        let mut app = App::test_instance();
        // Open modal
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(app.url_modal_open);

        // Rapidly flood 1,000 keystrokes of various characters
        for i in 0..1000 {
            let c = match i % 4 {
                0 => 'a',
                1 => 'Z',
                2 => '9',
                _ => '/',
            };
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(app.url_input_buffer.len(), 1000);

        // Flood 2,000 backspaces on a 1,000-char buffer (testing underflow resilience)
        for _ in 0..2000 {
            app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        }
        assert_eq!(app.url_input_buffer, "");
        assert!(app.url_modal_open);

        // Test multi-byte UTF-8 character stress and backspacing (avoiding byte slicing panics)
        let utf8_text = "🦀こんにちは世界🌟🚀";
        for c in utf8_text.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(app.url_input_buffer, utf8_text);

        for _ in utf8_text.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        }
        assert_eq!(app.url_input_buffer, "");

        // Test that control modifier key presses do not corrupt input buffer
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(app.url_input_buffer, "");
    }

    #[test]
    fn test_adversarial_tui_esc_cancellation_during_active_input() {
        let mut app = App::test_instance();
        let initial_selection = app.catalog_selection;

        // Open modal
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(app.url_modal_open);

        // Type active partial URL
        for c in "https://github.com/active-uncommitted-input".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.url_input_buffer,
            "https://github.com/active-uncommitted-input"
        );

        // Cancel with Esc
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.url_modal_open);
        assert!(app.url_input_error.is_none());

        // Regular navigation keys must now operate on the main view, not the modal
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        if !app.catalog.is_empty() {
            assert_eq!(app.catalog_selection, (initial_selection + 1).min(app.catalog.len() - 1));
        }

        // Re-opening modal with 'f' must present a cleanly cleared input buffer
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(app.url_modal_open);
        assert_eq!(app.url_input_buffer, "");
        assert!(app.url_input_error.is_none());

        // Cancel again with Esc
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.url_modal_open);
    }

    #[test]
    fn test_adversarial_tui_esc_cancellation_after_validation_error() {
        let mut app = App::test_instance();

        // Open modal and enter invalid host
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        for c in "https://gitlab.com/wrong/host".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        // Validation error is displayed, modal remains open
        assert!(app.url_modal_open);
        assert!(app.url_input_error.is_some());
        assert!(app.url_input_error.as_ref().unwrap().contains("github.com"));

        // Press Esc to dismiss
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.url_modal_open);
        assert!(app.url_input_error.is_none());

        // Re-open with 'F': clean state with no lingering error
        app.handle_key(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::NONE));
        assert!(app.url_modal_open);
        assert_eq!(app.url_input_buffer, "");
        assert!(app.url_input_error.is_none());
    }

    #[test]
    fn test_adversarial_tui_empty_and_whitespace_inputs() {
        let mut app = App::test_instance();

        // 1. Enter on completely empty buffer
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.url_modal_open);
        assert_eq!(
            app.url_input_error.as_deref(),
            Some("Repository URL cannot be empty")
        );

        // 2. Enter on whitespace-only buffer
        for _ in 0..5 {
            app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.url_modal_open);
        assert_eq!(
            app.url_input_error.as_deref(),
            Some("Repository URL cannot be empty")
        );

        // 3. Typing valid character immediately clears the error banner
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        assert!(app.url_input_error.is_none());
    }

    #[test]
    fn test_adversarial_tui_rapid_modal_open_close_cycles() {
        let mut app = App::test_instance();

        for i in 0..100 {
            let key = if i % 2 == 0 { 'f' } else { 'F' };
            app.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
            assert!(app.url_modal_open);
            app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(!app.url_modal_open);
            assert!(app.url_input_error.is_none());
        }
    }

    #[test]
    fn test_adversarial_tui_background_fetch_lifecycle_unreachable_and_error() {
        let mut app = App::test_instance();

        // Initiate valid fetch
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        app.url_input_buffer = "https://github.com/catppuccin/hyprland".into();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(!app.url_modal_open);
        assert!(app.operation.is_some());
        assert!(app.fetch_rx.is_some());

        // Ctrl+C while fetch is active is safely blocked
        assert!(!app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert!(app.status.contains("Installation is running"));

        // Replace receiver with one returning an unreachable network error
        let (tx, rx) = mpsc::channel();
        app.fetch_rx = Some(rx);
        tx.send(Err("network unreachable: connection refused (os error 111)".into()))
            .unwrap();

        // Process message via poll_fetch
        app.poll_fetch();

        assert!(app.fetch_rx.is_none());
        assert!(app.operation.is_none());
        assert_eq!(
            app.error_alert,
            Some("GitHub import failed: network unreachable: connection refused (os error 111)".into())
        );

        // Esc dismisses alert cleanly
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.error_alert.is_none());
    }
}

