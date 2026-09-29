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

    fn same_content(
        &self,
        other: &Self,
        same_image: &mut impl FnMut(&Path, &Path) -> bool,
    ) -> bool {
        match (self, other) {
            // Each copy is saved to its own file; the same picture copied
            // twice has identical bytes.
            (
                Self::Image {
                    path: a,
                    width: wa,
                    height: ha,
                },
                Self::Image {
                    path: b,
                    width: wb,
                    height: hb,
                },
            ) => (wa, ha) == (wb, hb) && (a == b || same_image(a, b)),
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
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Adds a copy to the top. Copying something already in the history moves
    /// it up instead of duplicating it. Returns files no longer referenced.
    pub fn push(&mut self, kind: ClipKind, source: Option<String>, now: u64) -> Vec<PathBuf> {
        self.push_with_image_comparison(kind, source, now, |a, b| a == b)
    }

    /// Adds a copy using the host-provided comparison for different image files.
    pub fn push_with_image_comparison(
        &mut self,
        kind: ClipKind,
        source: Option<String>,
        now: u64,
        mut same_image: impl FnMut(&Path, &Path) -> bool,
    ) -> Vec<PathBuf> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.kind.same_content(&kind, &mut same_image))
        {
            let mut existing = self.entries.remove(index).expect("index is in bounds");
            existing.copied_at = now;
            existing.source = source.or(existing.source);
            self.entries.push_front(existing);
            // A repeated image arrives as a fresh file the history won't use.
            return match (&kind, &self.entries[0].kind) {
                (ClipKind::Image { path: new, .. }, ClipKind::Image { path: kept, .. })
                    if new != kept =>
                {
                    vec![new.clone()]
                }
                _ => Vec::new(),
            };
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

// MARK: Browsing

/// The type filter in the history window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Text,
    Links,
    Images,
    Files,
}

impl Filter {
    pub const EVERY: [Filter; 5] = [
        Filter::All,
        Filter::Text,
        Filter::Links,
        Filter::Images,
        Filter::Files,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Text => "Text",
            Self::Links => "Links",
            Self::Images => "Images",
            Self::Files => "Files",
        }
    }

    pub fn matches(self, kind: &ClipKind) -> bool {
        matches!(
            (self, kind),
            (Self::All, _)
                | (Self::Text, ClipKind::Text { .. })
                | (Self::Links, ClipKind::Link { .. })
                | (Self::Images, ClipKind::Image { .. })
                | (Self::Files, ClipKind::File { .. })
        )
    }
}

impl ClipKind {
    pub fn type_label(&self) -> &'static str {
        match self {
            Self::Text { .. } => "Text",
            Self::Link { .. } => "Link",
            Self::Image { .. } => "Image",
            Self::File { .. } => "File",
        }
    }

    /// Everything a search can match on.
    fn searchable(&self) -> String {
        match self {
            Self::Text { text } => text.clone(),
            Self::Link { url } => url.clone(),
            Self::Image { width, height, .. } => format!("image {width}×{height} {width}x{height}"),
            Self::File { path } => path.to_string_lossy().into_owned(),
        }
    }
}

/// Entries matching `query` (case-insensitive, every word must appear) and
/// `filter`, newest first.
pub fn search<'a>(history: &'a History, query: &str, filter: Filter) -> Vec<&'a ClipEntry> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    history
        .entries
        .iter()
        .filter(|entry| filter.matches(&entry.kind))
        .filter(|entry| {
            if words.is_empty() {
                return true;
            }
            let mut haystack = entry.kind.searchable().to_lowercase();
            if let Some(source) = &entry.source {
                haystack.push(' ');
                haystack.push_str(&source.to_lowercase());
            }
            words.iter().all(|word| haystack.contains(word.as_str()))
        })
        .collect()
}

impl History {
    /// Removes one entry. Returns its image file, if it had one.
    pub fn remove(&mut self, id: u64) -> Option<Option<PathBuf>> {
        let index = self.entries.iter().position(|entry| entry.id == id)?;
        let entry = self.entries.remove(index)?;
        Some(match entry.kind {
            ClipKind::Image { path, .. } => Some(path),
            _ => None,
        })
    }
}

/// A day number in local time, for grouping.
fn local_day(secs: u64, utc_offset: i64) -> i64 {
    (secs as i64 + utc_offset).div_euclid(86_400)
}

/// "Today", "Yesterday" or "Earlier", as the history list groups entries.
pub fn day_group(copied_at: u64, now: u64, utc_offset: i64) -> &'static str {
    match local_day(now, utc_offset) - local_day(copied_at, utc_offset) {
        ..=0 => "Today",
        1 => "Yesterday",
        _ => "Earlier",
    }
}

/// "Sep 28, 2026 at 9:30 PM" in local time.
pub fn format_timestamp(secs: u64, utc_offset: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let local = secs as i64 + utc_offset;
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let minutes = local.rem_euclid(86_400) / 60;
    let (hour, minute) = (minutes / 60, minutes % 60);
    let (hour12, meridiem) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    format!(
        "{} {day}, {year} at {hour12}:{minute:02} {meridiem}",
        MONTHS[(month - 1) as usize]
    )
}

/// Proleptic Gregorian date for a count of days since 1970-01-01
/// (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Character and word counts for text previews.
pub fn text_counts(text: &str) -> (usize, usize) {
    (text.chars().count(), text.split_whitespace().count())
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
                    path: format!("/tmp/sidedoor-test-{n}.png").into(),
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

    fn history(kinds: Vec<ClipKind>) -> History {
        let mut history = History::default();
        for (n, kind) in kinds.into_iter().enumerate() {
            history.push(kind, Some("Notes".into()), n as u64);
        }
        history
    }

    #[test]
    fn search_matches_every_word_and_the_filter() {
        let history = history(vec![
            text("Standup notes: shipped the tint picker"),
            ClipKind::Link {
                url: "https://github.com/zed-industries/zed".into(),
            },
            text("Grocery list"),
            ClipKind::File {
                path: "/Users/me/Project brief.pdf".into(),
            },
        ]);
        let titles = |query: &str, filter| -> Vec<String> {
            search(&history, query, filter)
                .iter()
                .map(|e| e.kind.title())
                .collect()
        };
        assert_eq!(titles("", Filter::All).len(), 4);
        assert_eq!(
            titles("TINT standup", Filter::All),
            ["Standup notes: shipped the tint picker"]
        );
        assert_eq!(titles("zed", Filter::Text), Vec::<String>::new());
        assert_eq!(
            titles("", Filter::Links),
            ["https://github.com/zed-industries/zed"]
        );
        assert_eq!(titles("brief", Filter::Files), ["Project brief.pdf"]);
        // The source app is searchable too.
        assert_eq!(titles("notes", Filter::All).len(), 4);
    }

    #[test]
    fn removing_returns_the_image_to_delete() {
        let mut history = history(vec![
            text("a"),
            ClipKind::Image {
                path: "/tmp/x.png".into(),
                width: 2,
                height: 2,
            },
        ]);
        let image = history.entries[0].id;
        assert_eq!(
            history.remove(image),
            Some(Some(PathBuf::from("/tmp/x.png")))
        );
        assert_eq!(history.remove(image), None);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn groups_by_local_day() {
        // 2026-09-28 21:30 UTC; in UTC+2 that's 23:30 on the 28th.
        let now = 1_790_631_000;
        let offset = 2 * 3600;
        assert_eq!(day_group(now - 3600, now, offset), "Today");
        assert_eq!(day_group(now - 86_400, now, offset), "Yesterday");
        assert_eq!(day_group(now - 3 * 86_400, now, offset), "Earlier");
    }

    #[test]
    fn formats_local_timestamps() {
        assert_eq!(
            format_timestamp(1_790_631_000, 2 * 3600),
            "Sep 28, 2026 at 11:30 PM"
        );
        assert_eq!(format_timestamp(0, 0), "Jan 1, 1970 at 12:00 AM");
        assert_eq!(format_timestamp(951_782_400, 0), "Feb 29, 2000 at 12:00 AM");
    }

    #[test]
    fn counts_characters_and_words() {
        assert_eq!(text_counts("Shipped the new tint picker."), (28, 5));
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
