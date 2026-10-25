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
}
