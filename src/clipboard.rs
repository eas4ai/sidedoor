//! Clipboard history: what was copied, newest first, kept across restarts.

use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Entries kept in total.
pub const MAX_ENTRIES: usize = 200;
/// Images kept; older image entries are dropped with their files.
pub const MAX_IMAGES: usize = 12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClipKind {
    Text {
        text: String,
    },
    Link {
        url: String,
    },
    Image {
        path: PathBuf,
        width: u32,
        height: u32,
    },
    File {
        path: PathBuf,
    },
}

impl ClipKind {
    /// Text copied as a plain string becomes a link when it is one.
    pub fn from_text(text: String) -> Self {
        let trimmed = text.trim();
        let is_link = (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
            && !trimmed.contains(char::is_whitespace);
        if is_link {
            Self::Link {
                url: trimmed.to_string(),
            }
        } else {
            Self::Text { text }
        }
    }

    /// One-line summary for lists.
    pub fn title(&self) -> String {
        match self {
            Self::Text { text } => text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("")
                .to_string(),
            Self::Link { url } => url.clone(),
            Self::Image { width, height, .. } => format!("Image {width}×{height}"),
            Self::File { path } => path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into(),
            ),
        }
    }

    fn same_content(&self, other: &Self) -> bool {
        match (self, other) {
            // Each image copy is saved to its own file, so images never merge.
            (Self::Image { .. }, Self::Image { .. }) => false,
            _ => self == other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipEntry {
    pub id: u64,
    pub kind: ClipKind,
    pub source: Option<String>,
    /// Seconds since the Unix epoch.
    pub copied_at: u64,
}

impl ClipEntry {
    pub fn age_label(&self, now: u64) -> String {
        match now.saturating_sub(self.copied_at) {
            0..60 => "Just now".into(),
            secs if secs < 3600 => format!("{} min ago", secs / 60),
            secs if secs < 86_400 => format!("{} h ago", secs / 3600),
            secs => format!("{} d ago", secs / 86_400),
        }
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    pub entries: VecDeque<ClipEntry>,
    next_id: u64,
}

impl History {
    pub fn path() -> PathBuf {
        crate::platform::support_dir().join("clipboard.json")
    }

    pub fn image_dir() -> PathBuf {
        crate::platform::support_dir().join("clipboard-images")
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Adds a copy to the top. Copying something already in the history moves
    /// it up instead of duplicating it. Returns files no longer referenced.
    pub fn push(&mut self, kind: ClipKind, source: Option<String>, now: u64) -> Vec<PathBuf> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.kind.same_content(&kind))
        {
            let mut existing = self.entries.remove(index).expect("index is in bounds");
            existing.copied_at = now;
            existing.source = source.or(existing.source);
            self.entries.push_front(existing);
            return Vec::new();
        }
        self.next_id += 1;
        self.entries.push_front(ClipEntry {
            id: self.next_id,
            kind,
            source,
            copied_at: now,
        });
        self.trim()
    }

    /// Moves an entry to the top, as when it is copied again from the list.
    pub fn promote(&mut self, id: u64, now: u64) -> Option<&ClipEntry> {
        let index = self.entries.iter().position(|entry| entry.id == id)?;
        let mut entry = self.entries.remove(index)?;
        entry.copied_at = now;
        self.entries.push_front(entry);
        self.entries.front()
    }

    /// Empties the history. Returns image files to delete.
    pub fn clear(&mut self) -> Vec<PathBuf> {
        let files = self.image_files(0);
        self.entries.clear();
        files
    }

    fn image_files(&self, skip: usize) -> Vec<PathBuf> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.kind {
                ClipKind::Image { path, .. } => Some(path.clone()),
                _ => None,
            })
            .skip(skip)
            .collect()
    }

    fn trim(&mut self) -> Vec<PathBuf> {
        let stale_images = self.image_files(MAX_IMAGES);
        self.entries.retain(|entry| match &entry.kind {
            ClipKind::Image { path, .. } => !stale_images.contains(path),
            _ => true,
        });
        let mut removed = stale_images;
        while self.entries.len() > MAX_ENTRIES {
            if let Some(ClipEntry {
                kind: ClipKind::Image { path, .. },
                ..
            }) = self.entries.pop_back()
            {
                removed.push(path);
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> ClipKind {
        ClipKind::Text { text: value.into() }
    }

    #[test]
    fn newest_first_and_duplicates_move_up() {
        let mut history = History::default();
        history.push(text("a"), None, 1);
        history.push(text("b"), None, 2);
        history.push(text("a"), None, 3);
        let titles: Vec<_> = history.entries.iter().map(|e| e.kind.title()).collect();
        assert_eq!(titles, ["a", "b"]);
        assert_eq!(history.entries[0].copied_at, 3);
    }

    #[test]
    fn caps_entries_and_images() {
        let mut history = History::default();
        let mut removed = Vec::new();
        for n in 0..(MAX_IMAGES + 3) {
            removed.extend(history.push(
                ClipKind::Image {
                    path: format!("/tmp/sidekick-test-{n}.png").into(),
                    width: n as u32,
                    height: 1,
                },
                None,
                n as u64,
            ));
        }
        assert_eq!(history.len(), MAX_IMAGES);
        assert_eq!(removed.len(), 3);
        for n in 0..(MAX_ENTRIES + 10) {
            history.push(text(&n.to_string()), None, 0);
        }
        assert_eq!(history.len(), MAX_ENTRIES);
    }

    #[test]
    fn detects_links_and_summarizes() {
        assert_eq!(
            ClipKind::from_text(" https://github.com/zed ".into()),
            ClipKind::Link {
                url: "https://github.com/zed".into()
            }
        );
        assert!(matches!(
            ClipKind::from_text("see https://a.b now".into()),
            ClipKind::Text { .. }
        ));
        assert_eq!(text("\n  Standup notes\nMore").title(), "Standup notes");
    }

    #[test]
    fn promote_moves_to_top() {
        let mut history = History::default();
        history.push(text("a"), None, 1);
        history.push(text("b"), None, 2);
        let id = history.entries[1].id;
        assert_eq!(
            history.promote(id, 9).map(|e| e.kind.title()),
            Some("a".into())
        );
        assert_eq!(history.entries[0].copied_at, 9);
    }

    #[test]
    fn ages_read_naturally() {
        let entry = ClipEntry {
            id: 1,
            kind: text("a"),
            source: None,
            copied_at: 1000,
        };
        assert_eq!(entry.age_label(1030), "Just now");
        assert_eq!(entry.age_label(1000 + 125), "2 min ago");
        assert_eq!(entry.age_label(1000 + 7200), "2 h ago");
    }
}
