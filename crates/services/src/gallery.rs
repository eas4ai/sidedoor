//! The plugin gallery: plugins listed in Settings to install with a click.
//!
//! The list is `plugins/gallery.json` in the repository. A copy is built
//! in, so the gallery shows offline, and the app fetches the current one
//! from GitHub so new plugins appear without a release.
use serde::Deserialize;
use std::time::Duration;

const URL: &str = "https://raw.githubusercontent.com/lassejlv/sidedoor/main/plugins/gallery.json";
const BUNDLED: &str = include_str!("../../../plugins/gallery.json");

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Entry {
    pub name: String,
    pub description: String,
    /// An icon name, as in a plugin's `icon`.
    pub icon: String,
    /// Where to install it from, as pasted into Install from URL.
    pub link: String,
    /// The systems it works on (`macos`, `linux`, `windows`); all when empty.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// Made and kept up by the Sidedoor project.
    #[serde(default)]
    pub official: bool,
}

#[derive(Deserialize)]
struct Gallery {
    plugins: Vec<Entry>,
}

/// The gallery built into this version, for this system.
pub fn bundled() -> Vec<Entry> {
    parse(BUNDLED).unwrap_or_default()
}

/// The current gallery, for this system. Blocking; call it off the main
/// thread.
pub fn fetch() -> Result<Vec<Entry>, String> {
    let body = crate::http::agent(
        URL,
        ureq::AgentBuilder::new().timeout(Duration::from_secs(15)),
    )
    .get(URL)
    .call()
    .map_err(|err| err.to_string())?
    .into_string()
    .map_err(|err| err.to_string())?;
    parse(&body)
}

fn parse(json: &str) -> Result<Vec<Entry>, String> {
    let gallery: Gallery = serde_json::from_str(json).map_err(|err| err.to_string())?;
    Ok(gallery
        .plugins
        .into_iter()
        .filter(|entry| {
            entry.platforms.is_empty()
                || entry
                    .platforms
                    .iter()
                    .any(|platform| platform == std::env::consts::OS)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_gallery_reads() {
        let all: Gallery = serde_json::from_str(BUNDLED).unwrap();
        assert!(!all.plugins.is_empty());
        for entry in &all.plugins {
            assert!(entry.link.starts_with("https://"), "{}", entry.name);
        }
    }

    #[test]
    fn plugins_for_other_systems_are_left_out() {
        let json = r#"{"plugins": [
            {"name": "Here", "description": "", "icon": "a", "link": "x"},
            {"name": "Elsewhere", "description": "", "icon": "a", "link": "x",
             "platforms": ["plan9"]}
        ]}"#;
        let names: Vec<String> = parse(json).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Here"]);
    }
}
