#[derive(Debug, Clone, Default)]
pub struct UiState {
    pub title: String,
    pub selected_category: Option<String>,
    pub search: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub left: Option<String>,
    pub right: Option<String>,
}
pub fn unified_diff(left: &str, right: &str) -> Vec<DiffLine> {
    let a: Vec<_> = left.lines().map(str::to_string).collect();
    let b: Vec<_> = right.lines().map(str::to_string).collect();
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| DiffLine {
            left: a.get(i).cloned(),
            right: b.get(i).cloned(),
        })
        .collect()
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryItem {
    pub transaction: String,
    pub package: String,
    pub status: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteSwatch {
    pub name: String,
    pub hex: String,
}

impl PaletteSwatch {
    pub fn new(name: impl Into<String>, hex: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            hex: hex.into(),
        }
    }

    pub fn rgb(&self) -> Option<(u8, u8, u8)> {
        parse_hex_rgb(&self.hex)
    }
}

pub fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let clean = hex.trim().trim_start_matches('#');
    if !clean.is_ascii() {
        return None;
    }
    if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
        let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
        let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
        Some((r, g, b))
    } else if clean.len() == 3 {
        let r = u8::from_str_radix(&clean[0..1], 16).ok()? * 17;
        let g = u8::from_str_radix(&clean[1..2], 16).ok()? * 17;
        let b = u8::from_str_radix(&clean[2..3], 16).ok()? * 17;
        Some((r, g, b))
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffViewState {
    pub selected_file: usize,
    pub scroll_offset: usize,
    pub total_files: usize,
    pub total_lines: usize,
}

impl DiffViewState {
    pub fn new(total_files: usize, total_lines: usize) -> Self {
        Self {
            selected_file: 0,
            scroll_offset: 0,
            total_files,
            total_lines,
        }
    }

    pub fn next_file(&mut self, next_lines: usize) {
        if self.total_files > 0 {
            self.selected_file = (self.selected_file + 1) % self.total_files;
            self.scroll_offset = 0;
            self.total_lines = next_lines;
        }
    }

    pub fn prev_file(&mut self, prev_lines: usize) {
        if self.total_files > 0 {
            if self.selected_file == 0 {
                self.selected_file = self.total_files - 1;
            } else {
                self.selected_file -= 1;
            }
            self.scroll_offset = 0;
            self.total_lines = prev_lines;
        }
    }

    pub fn scroll_up(&mut self, step: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(step);
    }

    pub fn scroll_down(&mut self, step: usize) {
        let max = self.total_lines.saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + step).min(max);
    }

    pub fn scroll_home(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn scroll_end(&mut self) {
        self.scroll_offset = self.total_lines.saturating_sub(1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarouselModel {
    pub images: Vec<String>,
    pub selected: usize,
}

impl CarouselModel {
    pub fn new(images: Vec<String>) -> Self {
        Self {
            images,
            selected: 0,
        }
    }
    pub fn select(&mut self, index: usize) {
        if index < self.images.len() {
            self.selected = index;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPreview {
    pub package: String,
    pub files: usize,
    pub native_dependencies: Vec<String>,
    pub requires_authorization: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressModel {
    pub stage: String,
    pub percent: u8,
    pub message: String,
}

impl ProgressModel {
    pub fn new(stage: impl Into<String>, percent: u8, message: impl Into<String>) -> Self {
        Self {
            stage: stage.into(),
            percent: percent.min(100),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diff_keeps_synchronized_rows() {
        let rows = unified_diff("a\nb", "a\nc\nd");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].left.as_deref(), Some("b"));
        assert_eq!(rows[2].right.as_deref(), Some("d"));
    }

    #[test]
    fn carousel_selection_is_bounded() {
        let mut model = CarouselModel::new(vec!["a".into(), "b".into()]);
        model.select(1);
        model.select(9);
        assert_eq!(model.selected, 1);
    }

    #[test]
    fn preview_tracks_native_authorization() {
        let preview = InstallPreview {
            package: "fixture".into(),
            files: 2,
            native_dependencies: vec!["waybar".into()],
            requires_authorization: true,
        };
        assert!(preview.requires_authorization);
    }

    #[test]
    fn parse_hex_rgb_converts_six_and_three_char_hex() {
        assert_eq!(parse_hex_rgb("#1e1e2e"), Some((30, 30, 46)));
        assert_eq!(parse_hex_rgb("#89b4fa"), Some((137, 180, 250)));
        assert_eq!(parse_hex_rgb("#cba6f7"), Some((203, 166, 247)));
        assert_eq!(parse_hex_rgb("#cdd6f4"), Some((205, 214, 244)));
        assert_eq!(parse_hex_rgb("1e1e2e"), Some((30, 30, 46)));
        assert_eq!(parse_hex_rgb("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex_rgb("invalid"), None);
        assert_eq!(parse_hex_rgb("#12345"), None);
        assert_eq!(parse_hex_rgb(""), None);
    }

    #[test]
    fn parse_hex_rgb_rejects_non_ascii_and_multibyte_without_panic() {
        // Multi-byte characters matching byte length 6
        assert_eq!(parse_hex_rgb("🦀ab"), None);
        assert_eq!(parse_hex_rgb("#🦀ab"), None);
        assert_eq!(parse_hex_rgb("你好"), None);
        assert_eq!(parse_hex_rgb("#你好"), None);
        // Multi-byte characters matching byte length 3
        assert_eq!(parse_hex_rgb("你"), None);
        assert_eq!(parse_hex_rgb("#你"), None);
        // Emojis and other non-ASCII Unicode strings
        assert_eq!(parse_hex_rgb("🌟"), None);
        assert_eq!(parse_hex_rgb("#12345é"), None);
        assert_eq!(parse_hex_rgb("ééé"), None);
        assert_eq!(parse_hex_rgb("#ééé"), None);
    }

    #[test]
    fn palette_swatch_computes_rgb() {
        let swatch = PaletteSwatch::new("mocha_base", "#1e1e2e");
        assert_eq!(swatch.rgb(), Some((30, 30, 46)));
    }

    #[test]
    fn diff_view_state_file_cycling() {
        let mut state = DiffViewState::new(3, 100);
        assert_eq!(state.selected_file, 0);

        state.next_file(50);
        assert_eq!(state.selected_file, 1);
        assert_eq!(state.total_lines, 50);
        assert_eq!(state.scroll_offset, 0);

        state.next_file(20);
        assert_eq!(state.selected_file, 2);

        state.next_file(10);
        assert_eq!(state.selected_file, 0);

        state.prev_file(20);
        assert_eq!(state.selected_file, 2);

        state.prev_file(50);
        assert_eq!(state.selected_file, 1);
    }

    #[test]
    fn diff_view_state_scrolling_and_clamping() {
        let mut state = DiffViewState::new(1, 20);
        state.scroll_down(5);
        assert_eq!(state.scroll_offset, 5);

        state.scroll_down(30);
        assert_eq!(state.scroll_offset, 19);

        state.scroll_up(4);
        assert_eq!(state.scroll_offset, 15);

        state.scroll_home();
        assert_eq!(state.scroll_offset, 0);

        state.scroll_end();
        assert_eq!(state.scroll_offset, 19);
    }

    #[test]
    fn progress_model_clamps_percent() {
        let p = ProgressModel::new("Verify", 120, "Verifying artifacts");
        assert_eq!(p.percent, 100);
        let p2 = ProgressModel::new("Commit", 80, "Writing journal");
        assert_eq!(p2.percent, 80);
    }
}
