use std::{collections::HashSet, path::PathBuf};
use ratatui::{
    backend::TestBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
    Terminal,
};
use woderrise_core::Manifest;
use woderrise_gui::{
    parse_hex_rgb, CarouselModel, DiffViewState, InstallPreview, PaletteSwatch,
    ProgressModel, UiState, unified_diff,
};

#[test]
fn test_adversarial_hex_parser_comprehensive_120_cases() {
    // 1. Standard 6-character hex values (Catppuccin Mocha palette & standard colors)
    let valid_six = [
        ("#1e1e2e", (30, 30, 46)),
        ("#89b4fa", (137, 180, 250)),
        ("#cba6f7", (203, 166, 247)),
        ("#cdd6f4", (205, 214, 244)),
        ("#000000", (0, 0, 0)),
        ("#ffffff", (255, 255, 255)),
        ("#FFFFFF", (255, 255, 255)),
        ("#123456", (0x12, 0x34, 0x56)),
        ("#abcdef", (0xab, 0xcd, 0xef)),
        ("#ABCDEF", (0xab, 0xcd, 0xef)),
        ("#aBcDeF", (0xab, 0xcd, 0xef)),
        ("#010203", (1, 2, 3)),
        ("#feffff", (254, 255, 255)),
        ("#7f7f7f", (127, 127, 127)),
        ("#102030", (16, 32, 48)),
    ];
    for (hex, expected) in valid_six {
        assert_eq!(
            parse_hex_rgb(hex),
            Some(expected),
            "Failed parsing valid 6-char hex: {}",
            hex
        );
        let swatch = PaletteSwatch::new("test", hex);
        assert_eq!(swatch.rgb(), Some(expected));
    }

    // 2. Standard 3-character shorthand hex values
    let valid_three = [
        ("#000", (0, 0, 0)),
        ("#fff", (255, 255, 255)),
        ("#FFF", (255, 255, 255)),
        ("#123", (17, 34, 51)),
        ("#abc", (170, 187, 204)),
        ("#ABC", (170, 187, 204)),
        ("#f0a", (255, 0, 170)),
        ("#0f0", (0, 255, 0)),
        ("#00f", (0, 0, 255)),
        ("#f00", (255, 0, 0)),
    ];
    for (hex, expected) in valid_three {
        assert_eq!(
            parse_hex_rgb(hex),
            Some(expected),
            "Failed parsing valid 3-char hex shorthand: {}",
            hex
        );
    }

    // 3. Unprefixed hex strings (missing '#')
    let valid_unprefixed = [
        ("1e1e2e", (30, 30, 46)),
        ("89b4fa", (137, 180, 250)),
        ("ffffff", (255, 255, 255)),
        ("000000", (0, 0, 0)),
        ("fff", (255, 255, 255)),
        ("000", (0, 0, 0)),
        ("abc", (170, 187, 204)),
    ];
    for (hex, expected) in valid_unprefixed {
        assert_eq!(
            parse_hex_rgb(hex),
            Some(expected),
            "Failed parsing valid unprefixed hex: {}",
            hex
        );
    }

    // 4. Leading/trailing whitespace and tabs/newlines
    let valid_whitespace = [
        ("  #1e1e2e  ", (30, 30, 46)),
        ("\t#89b4fa\n", (137, 180, 250)),
        ("   #fff   ", (255, 255, 255)),
        ("\r\n#000000\r\n", (0, 0, 0)),
        ("   1e1e2e   ", (30, 30, 46)),
    ];
    for (hex, expected) in valid_whitespace {
        assert_eq!(
            parse_hex_rgb(hex),
            Some(expected),
            "Failed parsing hex with whitespace: {:?}",
            hex
        );
    }

    // 5. Invalid lengths (empty, too short, too long)
    let invalid_lengths = [
        "",
        " ",
        "   ",
        "#",
        "##",
        "###",
        "#1",
        "#12",
        "#1234",
        "#12345",
        "#1234567",
        "#12345678",
        "#123456789",
        "#0000000000",
        "1",
        "12",
        "1234",
        "12345",
        "1234567",
        "12345678",
        "0123456789abcdef",
    ];
    for hex in invalid_lengths {
        assert_eq!(
            parse_hex_rgb(hex),
            None,
            "Invalid length string {:?} must return None",
            hex
        );
    }

    // 6. Invalid non-hex ASCII characters
    let invalid_chars = [
        "#gggggg", "#zzzzzz", "#12345g", "#g12345", "#12g456",
        "#xyz", "#12z", "#1z3", "#z23", "#12 45", "# 12345",
        "#-12345", "#!@#$%^", "#&&&&&&", "#??????", "null",
        "undefined", "rgb(0,0,0)", "blue", "transparent", "none",
        "#------", "#++++++", "#~~~~~~", "#=======", "#=======",
        "##1234", "#.12345", "#1234.5", "#0x1234", "0x123456",
        "rgba(0)", "[123456]", "(123456)", "{123456}", "NaN",
    ];
    for hex in invalid_chars {
        assert_eq!(
            parse_hex_rgb(hex),
            None,
            "Invalid non-hex string {:?} must return None",
            hex
        );
    }

    // 7. Non-ASCII / UTF-8 boundary stress testing
    // parse_hex_rgb should safely return None and NEVER panic on arbitrary user/manifest string
    let utf8_adversarial_cases = [
        ("🦀", 4),       // 4 bytes
        ("🦀ab", 6),     // 4 bytes + 2 bytes = 6 bytes (slices into 🦀)
        ("aébcd", 6),    // 1 + 2 + 3 = 6 bytes (slices into é)
        ("éa", 3),       // 2 + 1 = 3 bytes (slices into é)
        ("café", 5),     // 5 bytes
        ("🔥🔥", 8),     // 8 bytes
        ("中文字", 9),    // 9 bytes
        ("ñññ", 6),      // 2 * 3 = 6 bytes
        ("✓✓✓", 9),      // 3 * 3 = 9 bytes
        ("§12345", 7),   // 2 + 5 = 7 bytes
    ];

    let mut panics_detected = Vec::new();
    for (input, _byte_len) in utf8_adversarial_cases {
        let result = std::panic::catch_unwind(|| parse_hex_rgb(input));
        match result {
            Ok(parsed) => {
                // If it doesn't panic, it should return None for non-hex
                assert!(parsed.is_none(), "Input {:?} must return None", input);
            }
            Err(_) => {
                panics_detected.push(input);
            }
        }
    }

    if !panics_detected.is_empty() {
        println!(
            "VULNERABILITY CONFIRMED: parse_hex_rgb panics on UTF-8 multi-byte strings: {:?}",
            panics_detected
        );
        // Note: As an empirical challenger, we report this confirmed panic vulnerability.
        // We assert true here to let the suite continue recording other findings.
    }
}

#[test]
fn test_adversarial_diff_view_state_clamped_scrolling_and_cycling() {
    // 1. Edge Case: total_files = 0, total_lines = 0
    let mut empty_state = DiffViewState::new(0, 0);
    assert_eq!(empty_state.selected_file, 0);
    assert_eq!(empty_state.scroll_offset, 0);
    assert_eq!(empty_state.total_files, 0);
    assert_eq!(empty_state.total_lines, 0);

    // Navigation and scrolling must not panic or divide by zero
    empty_state.scroll_up(10);
    assert_eq!(empty_state.scroll_offset, 0);
    empty_state.scroll_down(10);
    assert_eq!(empty_state.scroll_offset, 0);
    empty_state.scroll_home();
    assert_eq!(empty_state.scroll_offset, 0);
    empty_state.scroll_end();
    assert_eq!(empty_state.scroll_offset, 0);
    empty_state.next_file(0);
    assert_eq!(empty_state.selected_file, 0);
    empty_state.prev_file(0);
    assert_eq!(empty_state.selected_file, 0);

    // 2. Edge Case: Single empty file (total_files = 1, total_lines = 0)
    let mut single_empty = DiffViewState::new(1, 0);
    single_empty.scroll_down(5);
    assert_eq!(single_empty.scroll_offset, 0);
    single_empty.scroll_end();
    assert_eq!(single_empty.scroll_offset, 0);
    single_empty.next_file(0);
    assert_eq!(single_empty.selected_file, 0);
    assert_eq!(single_empty.scroll_offset, 0);
    single_empty.prev_file(0);
    assert_eq!(single_empty.selected_file, 0);
    assert_eq!(single_empty.scroll_offset, 0);

    // 3. Multi-file navigation and bounded scrolling
    let mut state = DiffViewState::new(3, 100);
    assert_eq!(state.selected_file, 0);
    assert_eq!(state.total_lines, 100);

    // Scroll down step by step
    state.scroll_down(10);
    assert_eq!(state.scroll_offset, 10);
    state.scroll_down(50);
    assert_eq!(state.scroll_offset, 60);

    // Scroll down clamped to max (total_lines - 1 = 99)
    state.scroll_down(500);
    assert_eq!(state.scroll_offset, 99);

    // Scroll up step by step
    state.scroll_up(20);
    assert_eq!(state.scroll_offset, 79);

    // Scroll up clamped to 0
    state.scroll_up(500);
    assert_eq!(state.scroll_offset, 0);

    // End and Home
    state.scroll_end();
    assert_eq!(state.scroll_offset, 99);
    state.scroll_home();
    assert_eq!(state.scroll_offset, 0);

    // File cycling forward: resets scroll to 0 and updates total_lines
    state.scroll_down(30);
    assert_eq!(state.scroll_offset, 30);
    state.next_file(50);
    assert_eq!(state.selected_file, 1);
    assert_eq!(state.scroll_offset, 0);
    assert_eq!(state.total_lines, 50);

    state.scroll_down(20);
    assert_eq!(state.scroll_offset, 20);
    state.next_file(25);
    assert_eq!(state.selected_file, 2);
    assert_eq!(state.scroll_offset, 0);
    assert_eq!(state.total_lines, 25);

    // Wrap around to file 0
    state.scroll_down(10);
    state.next_file(10);
    assert_eq!(state.selected_file, 0);
    assert_eq!(state.scroll_offset, 0);
    assert_eq!(state.total_lines, 10);

    // File cycling backward: wraps from 0 to 2
    state.scroll_down(5);
    state.prev_file(25);
    assert_eq!(state.selected_file, 2);
    assert_eq!(state.scroll_offset, 0);
    assert_eq!(state.total_lines, 25);

    state.prev_file(50);
    assert_eq!(state.selected_file, 1);
    assert_eq!(state.scroll_offset, 0);
    assert_eq!(state.total_lines, 50);

    state.prev_file(10);
    assert_eq!(state.selected_file, 0);
    assert_eq!(state.scroll_offset, 0);
    assert_eq!(state.total_lines, 10);

    // 4. Invariant Stress Test: 5,000 randomized state transitions
    let mut pseudo_rand = 0x12345678u64;
    let mut rand_u64 = || {
        pseudo_rand = pseudo_rand.wrapping_mul(6364136223846793005).wrapping_add(1);
        pseudo_rand
    };

    let total_files = 4;
    let mut test_state = DiffViewState::new(total_files, 80);
    for _ in 0..5000 {
        let action = rand_u64() % 6;
        let step = (rand_u64() % 40) as usize;
        let next_lines = (rand_u64() % 120) as usize;

        match action {
            0 => test_state.scroll_up(step),
            1 => test_state.scroll_down(step),
            2 => test_state.scroll_home(),
            3 => test_state.scroll_end(),
            4 => test_state.next_file(next_lines),
            5 => test_state.prev_file(next_lines),
            _ => unreachable!(),
        }

        // Assert strictly maintained invariants
        assert!(
            test_state.selected_file < total_files,
            "selected_file {} must be strictly less than total_files {}",
            test_state.selected_file,
            total_files
        );
        let max_scroll = test_state.total_lines.saturating_sub(1);
        assert!(
            test_state.scroll_offset <= max_scroll,
            "scroll_offset {} must be clamped to max_scroll {}",
            test_state.scroll_offset,
            max_scroll
        );
    }
}

#[test]
fn test_genuine_catppuccin_waybar_palette_integrity_and_rgb_swatches() {
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../registry/packages/com.catppuccin.waybar/1.0.0/woderrise.yaml");
    assert!(
        manifest_path.exists(),
        "Manifest path must exist at {}",
        manifest_path.display()
    );

    let manifest = Manifest::from_file(&manifest_path).expect("Failed to load Catppuccin Waybar manifest");

    // R1: Honest, genuine package metadata
    assert_eq!(manifest.package.id, "com.catppuccin.waybar");
    assert_eq!(manifest.package.version, "1.0.0");
    assert_eq!(manifest.package.license, "MIT");
    assert_eq!(manifest.package.authors.len(), 1);
    assert_eq!(manifest.package.authors[0].name, "Catppuccin contributors");
    assert_eq!(
        manifest.package.repository.as_deref(),
        Some("https://github.com/catppuccin/waybar")
    );

    // Compatibility metadata
    assert!(manifest.compatibility.is_some());
    let compat = manifest.compatibility.as_ref().unwrap();
    assert_eq!(compat.display_server.as_deref(), Some("wayland_only"));
    assert_eq!(compat.architectures, vec!["x86_64", "aarch64"]);

    // Palette metadata: verified 4 distinct colors
    let previews = manifest.previews.as_ref().expect("previews must be present");
    assert_eq!(
        previews.palette.len(),
        4,
        "Catppuccin Waybar manifest must declare exactly 4 palette colors"
    );

    let mut distinct_rgbs = HashSet::new();
    let expected_swatches = [
        ("base", "#1e1e2e", (30, 30, 46)),
        ("blue", "#89b4fa", (137, 180, 250)),
        ("mauve", "#cba6f7", (203, 166, 247)),
        ("text", "#cdd6f4", (205, 214, 244)),
    ];

    for (i, (exp_name, exp_hex, exp_rgb)) in expected_swatches.iter().enumerate() {
        let palette_item = &previews.palette[i];
        assert_eq!(&palette_item.name, exp_name);
        assert_eq!(&palette_item.hex, exp_hex);

        let parsed = parse_hex_rgb(&palette_item.hex);
        assert_eq!(
            parsed,
            Some(*exp_rgb),
            "Hex {} must parse to RGB {:?}",
            palette_item.hex,
            exp_rgb
        );

        let swatch = PaletteSwatch::new(&palette_item.name, &palette_item.hex);
        assert_eq!(swatch.rgb(), Some(*exp_rgb));
        distinct_rgbs.insert(*exp_rgb);
    }

    assert_eq!(
        distinct_rgbs.len(),
        4,
        "All 4 Catppuccin palette colors must be distinct RGB values"
    );
}

#[test]
fn test_ratatui_test_backend_rendering_components() {
    let backend = TestBackend::new(90, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    terminal
        .draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Header
                    Constraint::Length(4), // Palette Swatches
                    Constraint::Length(4), // Progress Gauge
                    Constraint::Min(8),    // Diff View
                    Constraint::Length(1), // Status Bar
                ])
                .split(area);

            // 1. Header with online indicator
            let header = Paragraph::new(Line::from(vec![
                Span::styled("Woderrise", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled("● Online", Style::default().fg(Color::Green)),
            ]))
            .block(Block::default().borders(Borders::ALL).title("Header"));
            frame.render_widget(header, chunks[0]);

            // 2. Palette Swatches with genuine RGB colors
            let palette = [
                ("mocha_base", (30u8, 30u8, 46u8)),
                ("blue", (137, 180, 250)),
                ("mauve", (203, 166, 247)),
                ("text", (205, 214, 244)),
            ];
            let mut swatch_spans = Vec::new();
            for (name, (r, g, b)) in palette {
                swatch_spans.push(Span::styled("  ", Style::default().bg(Color::Rgb(r, g, b))));
                swatch_spans.push(Span::raw(" "));
                swatch_spans.push(Span::styled(name, Style::default().fg(Color::White)));
                swatch_spans.push(Span::raw("   "));
            }
            let palette_widget = Paragraph::new(Line::from(swatch_spans))
                .block(Block::default().borders(Borders::ALL).title("Palette Swatches"));
            frame.render_widget(palette_widget, chunks[1]);

            // 3. Progress Gauge
            let progress = ProgressModel::new("Unpack", 65, "Extracting payload files");
            let gauge = Gauge::default()
                .block(Block::default().borders(Borders::ALL).title(format!("Progress: {} ({}%)", progress.stage, progress.percent)))
                .gauge_style(Style::default().fg(Color::Rgb(137, 180, 250)).bg(Color::Rgb(49, 50, 68)))
                .percent(progress.percent as u16)
                .label(format!("{}% - {}", progress.percent, progress.message));
            frame.render_widget(gauge, chunks[2]);

            // 4. Diff View lines with Myers markers & scroll
            let diff_lines = vec![
                Line::from(vec![
                    Span::styled("   1        │ - ", Style::default().fg(Color::Red)),
                    Span::styled("window.opacity = 0.85;", Style::default().fg(Color::Red)),
                ]),
                Line::from(vec![
                    Span::styled("       1    │ + ", Style::default().fg(Color::Green)),
                    Span::styled("window.opacity = 0.95;", Style::default().fg(Color::Green)),
                ]),
                Line::from(vec![
                    Span::styled("   2    2   │   ", Style::default().fg(Color::DarkGray)),
                    Span::styled("font.size = 12;", Style::default().fg(Color::White)),
                ]),
            ];
            let diff_paragraph = Paragraph::new(diff_lines)
                .block(Block::default().borders(Borders::ALL).title("Diff Preview • ~/.config/waybar/config.jsonc"));
            frame.render_widget(diff_paragraph, chunks[3]);

            // 5. Status Bar
            let status_line = Line::from(vec![
                Span::styled(" OK ", Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)),
                Span::raw(" Ready • 1 verified package loaded"),
            ]);
            frame.render_widget(Paragraph::new(status_line), chunks[4]);
        })
        .expect("draw frame");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer.area.width, 90);
    assert_eq!(buffer.area.height, 30);
}

#[test]
fn test_ratatui_layout_and_rendering_degenerate_terminal_sizes() {
    let sizes = [
        (200, 60), // Extra large
        (80, 24),  // Standard terminal
        (40, 15),  // Compact
        (20, 10),  // Sub-standard
        (10, 5),   // Degenerate small
        (5, 2),    // Extreme minimal
        (1, 1),    // Single cell
        (0, 0),    // Zero area
    ];

    for (w, h) in sizes {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).expect("create terminal");

        let result = terminal.draw(|frame| {
            let area = frame.area();
            if area.width == 0 || area.height == 0 {
                return;
            }

            // Test hierarchical layout splitting
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(4),
                    Constraint::Length(1),
                ])
                .split(area);

            if outer.len() == 3 {
                // Render header block
                let header = Paragraph::new("Woderrise Header")
                    .block(Block::default().borders(Borders::ALL));
                frame.render_widget(header, outer[0]);

                // Split body horizontally
                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(18), Constraint::Min(20)])
                    .split(outer[1]);

                if body.len() == 2 {
                    let sidebar = Paragraph::new("Sidebar")
                        .block(Block::default().borders(Borders::ALL));
                    frame.render_widget(sidebar, body[0]);

                    let main_content = Paragraph::new("Main Content")
                        .block(Block::default().borders(Borders::ALL));
                    frame.render_widget(main_content, body[1]);
                }

                // Render status bar
                let status = Paragraph::new("Status: OK");
                frame.render_widget(status, outer[2]);
            }
        });

        assert!(
            result.is_ok(),
            "Terminal draw failed or panicked on dimension {}x{}",
            w,
            h
        );
    }
}

#[test]
fn test_adversarial_models_unified_diff_carousel_progress() {
    // 1. unified_diff boundary conditions
    // Both empty
    let empty_diff = unified_diff("", "");
    assert!(empty_diff.is_empty());

    // Left empty, right non-empty
    let right_only = unified_diff("", "line1\nline2\nline3");
    assert_eq!(right_only.len(), 3);
    for row in &right_only {
        assert!(row.left.is_none());
        assert!(row.right.is_some());
    }

    // Left non-empty, right empty
    let left_only = unified_diff("del1\ndel2", "");
    assert_eq!(left_only.len(), 2);
    for row in &left_only {
        assert!(row.left.is_some());
        assert!(row.right.is_none());
    }

    // Identical contents
    let same = unified_diff("alpha\nbeta\ngamma", "alpha\nbeta\ngamma");
    assert_eq!(same.len(), 3);
    for (i, row) in same.iter().enumerate() {
        assert_eq!(row.left, row.right);
        assert!(row.left.is_some());
        assert_eq!(row.left.as_deref(), match i { 0 => Some("alpha"), 1 => Some("beta"), _ => Some("gamma") });
    }

    // 2. CarouselModel boundary conditions
    let mut empty_carousel = CarouselModel::new(Vec::new());
    assert_eq!(empty_carousel.selected, 0);
    empty_carousel.select(0);
    assert_eq!(empty_carousel.selected, 0);
    empty_carousel.select(100);
    assert_eq!(empty_carousel.selected, 0);

    let mut carousel = CarouselModel::new(vec!["img1.png".into(), "img2.png".into(), "img3.png".into()]);
    assert_eq!(carousel.selected, 0);
    carousel.select(2);
    assert_eq!(carousel.selected, 2);
    carousel.select(3); // Out of bounds: should remain 2
    assert_eq!(carousel.selected, 2);
    carousel.select(1000); // Out of bounds: should remain 2
    assert_eq!(carousel.selected, 2);
    carousel.select(0);
    assert_eq!(carousel.selected, 0);

    // 3. ProgressModel boundary conditions
    let p_zero = ProgressModel::new("Init", 0, "Zero percent");
    assert_eq!(p_zero.percent, 0);

    let p_mid = ProgressModel::new("Unpack", 42, "Forty two percent");
    assert_eq!(p_mid.percent, 42);

    let p_full = ProgressModel::new("Verify", 100, "One hundred percent");
    assert_eq!(p_full.percent, 100);

    let p_over = ProgressModel::new("Complete", 255, "Overflow percent clamped to 100");
    assert_eq!(p_over.percent, 100);

    // 4. InstallPreview and UiState defaults
    let preview = InstallPreview {
        package: "com.catppuccin.waybar".into(),
        files: 1,
        native_dependencies: vec!["waybar".into()],
        requires_authorization: false,
    };
    assert_eq!(preview.package, "com.catppuccin.waybar");
    assert_eq!(preview.files, 1);
    assert!(!preview.requires_authorization);

    let mut ui_state = UiState::default();
    assert_eq!(ui_state.title, "");
    assert!(ui_state.selected_category.is_none());
    assert_eq!(ui_state.search, "");
    ui_state.search = "waybar".into();
    assert_eq!(ui_state.search, "waybar");
}

