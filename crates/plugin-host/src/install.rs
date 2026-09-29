//! Installing plugins from GitHub: download a repository at one commit, find
//! the plugin inside it, and move it into the plugins folder. Nothing in it
//! runs until the user adds it to the dock.

use crate::{Manifest, sdk::slug};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// Kept in an installed plugin's folder: where it came from, for updates.
pub const SOURCE_FILE: &str = ".sidedoor-source.json";
/// Downloads stop here; a plugin is a few source files.
const MAX_DOWNLOAD: u64 = 50 * 1024 * 1024;
const API: &str = "https://api.github.com";

/// A plugin on GitHub: a repository, optionally a branch or tag, and the
/// folder in it that holds the plugin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHub {
    pub owner: String,
    pub repo: String,
    /// A branch, tag or commit; the default branch when missing.
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// The plugin's folder inside the repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl GitHub {
    /// Reads what people paste: `owner/repo`, `github.com/owner/repo`, a
    /// clone URL, or a link to a folder like `…/tree/main/plugins/timer`.
    pub fn parse(input: &str) -> Result<Self, String> {
        const HINT: &str = "Paste a GitHub link, like github.com/owner/repo.";
        let mut text = input.trim().trim_end_matches('/');
        for prefix in ["https://", "http://", "git@github.com:", "www."] {
            text = text.strip_prefix(prefix).unwrap_or(text);
        }
        if text.is_empty() {
            return Err(HINT.into());
        }
        let rest = match text.strip_prefix("github.com/") {
            Some(rest) => rest,
            // A bare `owner/repo` has no dot before its first slash.
            None if !text.split('/').next().unwrap_or_default().contains('.') => text,
            None => return Err("Only GitHub links can be installed.".into()),
        };
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
        let [owner, repo, more @ ..] = parts.as_slice() else {
            return Err(HINT.into());
        };
        let repo = repo.trim_end_matches(".git");
        let name_ok = |name: &str| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        if !name_ok(owner) || !name_ok(repo) {
            return Err(HINT.into());
        }
        let (reference, path) = match more {
            [] => (None, None),
            [kind @ ("tree" | "blob"), reference, path @ ..] => {
                let mut path = path.to_vec();
                // A link to a file, like …/blob/main/timer/index.tsx, means its folder.
                if *kind == "blob" && path.last().is_some_and(|last| last.contains('.')) {
                    path.pop();
                }
                if path.iter().any(|part| *part == ".." || *part == ".") {
                    return Err(HINT.into());
                }
                let path = (!path.is_empty()).then(|| path.join("/"));
                (Some((*reference).to_string()), path)
            }
            _ => return Err(HINT.into()),
        };
        Ok(Self {
            owner: (*owner).to_string(),
            repo: repo.to_string(),
            reference,
            path,
        })
    }

    /// `owner/repo`, or `owner/repo/folder` for a plugin inside it.
    pub fn label(&self) -> String {
        match &self.path {
            Some(path) => format!("{}/{}/{path}", self.owner, self.repo),
            None => format!("{}/{}", self.owner, self.repo),
        }
    }

    /// The page on github.com.
    pub fn url(&self) -> String {
        let mut url = format!("https://github.com/{}/{}", self.owner, self.repo);
        if self.reference.is_some() || self.path.is_some() {
            url.push_str("/tree/");
            url.push_str(self.reference.as_deref().unwrap_or("HEAD"));
            if let Some(path) = &self.path {
                url.push('/');
                url.push_str(path);
            }
        }
        url
    }

    /// Whether both name the same plugin, on any branch.
    pub fn same_plugin(&self, other: &Self) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner)
            && self.repo.eq_ignore_ascii_case(&other.repo)
            && self.path == other.path
    }
}

/// Where an installed plugin came from, and at which commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    #[serde(flatten)]
    pub github: GitHub,
    pub commit: String,
}

impl Source {
    pub fn read(dir: &Path) -> Option<Self> {
        serde_json::from_str(&fs::read_to_string(dir.join(SOURCE_FILE)).ok()?).ok()
    }

    fn write(&self, dir: &Path) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(dir.join(SOURCE_FILE), json)
    }

    /// The commit as GitHub shows it, e.g. `3f2a9c1`.
    pub fn short_commit(&self) -> &str {
        self.commit.get(..7).unwrap_or(&self.commit)
    }
}

/// A downloaded plugin, waiting for the user to trust it. Dropping it
/// deletes the download.
#[derive(Debug)]
pub struct Staged {
    /// Read from its source without running it.
    pub manifest: Manifest,
    pub source: Source,
    /// The folder name it gets in the plugins folder.
    pub id: String,
    /// The download, removed on drop.
    pub root: PathBuf,
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Downloads plugins; blocking, so the app calls it off the main thread.
pub trait Installer: Send + Sync {
    /// Downloads `source` at its newest commit and gets it ready to install.
    fn fetch(&self, source: &GitHub) -> Result<Staged, String>;
    /// The newest commit of `source`, to tell whether an update exists.
    fn latest_commit(&self, source: &GitHub) -> Result<String, String>;
}

/// Downloads from GitHub into a folder beside the plugins, so installing
/// is a rename.
pub struct GitHubInstaller {
    pub plugins: PathBuf,
    /// Installs a plugin's npm dependencies, when it has any.
    pub bun: Option<PathBuf>,
}

impl Installer for GitHubInstaller {
    fn fetch(&self, source: &GitHub) -> Result<Staged, String> {
        let commit = self.latest_commit(source)?;
        let url = format!(
            "{API}/repos/{}/{}/tarball/{commit}",
            source.owner, source.repo
        );
        let response = agent()
            .get(&url)
            .call()
            .map_err(|err| describe(err, source))?;
        let root = staging_dir(&self.plugins).map_err(|err| err.to_string())?;
        // Removes the download if anything below fails.
        let cleanup = Cleanup(root.clone());
        unpack(response.into_reader(), &root)
            .map_err(|err| format!("Couldn't unpack {}: {err}", source.label()))?;
        let dir = locate(&root, source)?;
        install_dependencies(&dir, self.bun.as_deref())?;
        let manifest = Manifest::read(&dir).ok_or("The plugin can't be read.")?;
        let id = slug(if dir.parent() == Some(root.as_path()) {
            &source.repo
        } else {
            &manifest.id
        });
        std::mem::forget(cleanup);
        Ok(Staged {
            manifest,
            source: Source {
                github: source.clone(),
                commit,
            },
            id,
            root,
        })
    }

    fn latest_commit(&self, source: &GitHub) -> Result<String, String> {
        let reference = source.reference.as_deref().unwrap_or("HEAD");
        let url = format!(
            "{API}/repos/{}/{}/commits/{reference}",
            source.owner, source.repo
        );
        let sha = agent()
            .get(&url)
            .set("Accept", "application/vnd.github.sha")
            .call()
            .map_err(|err| describe(err, source))?
            .into_string()
            .map_err(|err| format!("Couldn't read GitHub's answer: {err}"))?;
        let sha = sha.trim();
        if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("GitHub didn't say which commit is newest.".into());
        }
        Ok(sha.to_string())
    }
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .user_agent("Sidedoor")
        .build()
}

fn describe(err: ureq::Error, source: &GitHub) -> String {
    match err {
        ureq::Error::Status(404 | 422, _) => format!(
            "Couldn't find {} on GitHub. Check the link, and that the repository is public.",
            source.url().trim_start_matches("https://")
        ),
        ureq::Error::Status(403 | 429, _) => {
            "GitHub is limiting downloads right now. Try again in a few minutes.".into()
        }
        ureq::Error::Status(code, _) => format!("GitHub answered with an error ({code})."),
        ureq::Error::Transport(err) => format!("Couldn't reach GitHub: {err}"),
    }
}

/// A new, empty folder under `plugins/.installing`. The dot keeps it out of
/// discovery and reloads.
fn staging_dir(plugins: &Path) -> io::Result<PathBuf> {
    let parent = plugins.join(".installing");
    fs::create_dir_all(&parent)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let dir = parent.join(format!("{}-{stamp}", std::process::id()));
    fs::create_dir(&dir)?;
    Ok(dir)
}

/// Unpacks a gzipped tarball into `root`. Entries can't escape `root`.
pub(crate) fn unpack(reader: impl Read, root: &Path) -> io::Result<()> {
    let mut limited = reader.take(MAX_DOWNLOAD + 1);
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&mut limited));
    archive.set_preserve_permissions(false);
    archive.unpack(root)?;
    if limited.limit() == 0 {
        return Err(io::Error::other("the download is larger than 50 MB"));
    }
    Ok(())
}

fn is_plugin_dir(entry: &fs::DirEntry) -> bool {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    entry.file_type().is_ok_and(|kind| kind.is_dir())
        && !name.starts_with('.')
        && name != "node_modules"
}

/// The plugin folder in an unpacked repository.
pub(crate) fn locate(root: &Path, source: &GitHub) -> Result<PathBuf, String> {
    // GitHub's tarballs hold one folder, named like "owner-repo-3f2a9c1".
    let top = fs::read_dir(root)
        .map_err(|err| err.to_string())?
        .flatten()
        .find(is_plugin_dir)
        .map(|entry| entry.path())
        .ok_or("The download was empty.")?;
    let base = match &source.path {
        Some(path) => top.join(path),
        None => top.clone(),
    };
    if !base.is_dir() {
        return Err(format!(
            "{}/{} has no folder “{}”.",
            source.owner,
            source.repo,
            source.path.as_deref().unwrap_or_default()
        ));
    }
    if Manifest::read(&base).is_some() {
        return Ok(base);
    }
    // Look a couple of levels down, as in a repo of several plugins.
    let mut found = Vec::new();
    let mut folders = vec![(base.clone(), 0)];
    while let Some((dir, depth)) = folders.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            if !is_plugin_dir(&entry) {
                continue;
            }
            if Manifest::read(&entry.path()).is_some() {
                found.push(entry.path());
            } else if depth < 1 {
                folders.push((entry.path(), depth + 1));
            }
        }
    }
    found.sort();
    match found.as_slice() {
        [] => Err(format!(
            "There's no Sidedoor plugin in {}. A plugin is a folder with an index.tsx \
             that calls definePlugin.",
            source.label()
        )),
        [only] => Ok(only.clone()),
        several => {
            let relative = |path: &PathBuf| {
                path.strip_prefix(&top)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/")
            };
            let names: Vec<String> = several.iter().map(relative).collect();
            let example = GitHub {
                reference: source.reference.clone(),
                path: Some(names[0].clone()),
                ..source.clone()
            };
            Err(format!(
                "{} has several plugins: {}. Paste the link to the one you want, like {}.",
                source.label(),
                names.join(", "),
                example.url().trim_start_matches("https://")
            ))
        }
    }
}

/// Installs npm packages the plugin depends on. Its scripts don't run.
fn install_dependencies(dir: &Path, bun: Option<&Path>) -> Result<(), String> {
    let Ok(json) = fs::read_to_string(dir.join("package.json")) else {
        return Ok(());
    };
    let package: serde_json::Value = serde_json::from_str(&json)
        .map_err(|err| format!("The plugin's package.json can't be read: {err}"))?;
    let needed = package
        .get("dependencies")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|dependencies| dependencies.keys().any(|name| name != "@sidedoor/sdk"));
    if !needed {
        return Ok(());
    }
    let bun = bun.ok_or("This plugin has dependencies, and installing them needs Bun.")?;
    let mut command = Command::new(bun);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .args(["install", "--production", "--ignore-scripts"])
        .current_dir(dir)
        .output()
        .map_err(|err| format!("Couldn't install its dependencies: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = stderr
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("bun install failed");
    Err(format!(
        "Couldn't install its dependencies: {}",
        reason.trim()
    ))
}

/// Moves a downloaded plugin into `plugins` and returns it as found there.
/// An earlier install of the same plugin is replaced in place, keeping its
/// folder name, so its dock item, settings and saved data carry over.
pub fn install(staged: Staged, plugins: &Path) -> io::Result<Manifest> {
    fs::create_dir_all(plugins)?;
    let dir = staged.manifest.dir.clone();
    let mut target = plugins.join(&staged.id);
    let mut n = 2;
    let replacing = loop {
        if !target.exists() {
            break false;
        }
        if Source::read(&target).is_some_and(|old| old.github.same_plugin(&staged.source.github)) {
            break true;
        }
        target = plugins.join(format!("{}-{n}", staged.id));
        n += 1;
    };
    staged.source.write(&dir)?;
    if replacing {
        let previous = staged.root.join(".previous");
        fs::rename(&target, &previous)?;
        if let Err(err) = fs::rename(&dir, &target) {
            let _ = fs::rename(&previous, &target);
            return Err(err);
        }
    } else {
        fs::rename(&dir, &target)?;
    }
    // Dropping `staged` removes what's left of the download.
    Manifest::read(&target).ok_or_else(|| io::Error::other("the installed plugin can't be read"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn github(owner: &str, repo: &str, reference: Option<&str>, path: Option<&str>) -> GitHub {
        GitHub {
            owner: owner.into(),
            repo: repo.into(),
            reference: reference.map(Into::into),
            path: path.map(Into::into),
        }
    }

    #[test]
    fn parses_the_links_people_paste() {
        let plain = github("lasse", "timer", None, None);
        for input in [
            "lasse/timer",
            "github.com/lasse/timer",
            "https://github.com/lasse/timer",
            "https://www.github.com/lasse/timer/",
            "https://github.com/lasse/timer.git",
            "git@github.com:lasse/timer.git",
            "  https://github.com/lasse/timer?tab=readme  ",
        ] {
            assert_eq!(GitHub::parse(input), Ok(plain.clone()), "{input}");
        }
        assert_eq!(
            GitHub::parse("https://github.com/lasse/widgets/tree/main/plugins/timer"),
            Ok(github(
                "lasse",
                "widgets",
                Some("main"),
                Some("plugins/timer")
            ))
        );
        assert_eq!(
            GitHub::parse("github.com/lasse/widgets/blob/v2/timer/index.tsx"),
            Ok(github("lasse", "widgets", Some("v2"), Some("timer")))
        );
        assert_eq!(
            GitHub::parse("github.com/lasse/widgets/tree/dev"),
            Ok(github("lasse", "widgets", Some("dev"), None))
        );
        for bad in [
            "",
            "lasse",
            "https://gitlab.com/lasse/timer",
            "github.com/lasse/timer/issues",
            "github.com/lasse/timer/tree/main/../x",
            "github.com/la sse/timer",
        ] {
            assert!(GitHub::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn links_round_trip() {
        let source = github("lasse", "widgets", None, Some("plugins/timer"));
        assert_eq!(
            source.url(),
            "https://github.com/lasse/widgets/tree/HEAD/plugins/timer"
        );
        assert_eq!(GitHub::parse(&source.url()).unwrap().path, source.path);
        assert_eq!(source.label(), "lasse/widgets/plugins/timer");
        assert!(source.same_plugin(&github(
            "Lasse",
            "Widgets",
            Some("dev"),
            Some("plugins/timer")
        )));
        assert!(!source.same_plugin(&github("lasse", "widgets", None, None)));
    }

    const PLUGIN: &str = r#"export default definePlugin({ name: "Timer", icon: "timer" });"#;

    /// A gzipped tarball shaped like GitHub's: one top folder.
    fn tarball(files: &[(&str, &str)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, contents) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(
                    &mut header,
                    format!("lasse-repo-3f2a9c1/{path}"),
                    contents.as_bytes(),
                )
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sidedoor-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn staged(root: &Path, source: GitHub, commit: &str) -> Staged {
        let dir = locate(root, &source).unwrap();
        Staged {
            manifest: Manifest::read(&dir).unwrap(),
            source: Source {
                github: source,
                commit: commit.into(),
            },
            id: "timer".into(),
            root: root.to_path_buf(),
        }
    }

    #[test]
    fn finds_the_plugin_at_the_root_in_a_folder_or_asks_which() {
        let dir = scratch("locate");
        let root = dir.join("root");
        fs::create_dir(&root).unwrap();
        unpack(&tarball(&[("index.tsx", PLUGIN)])[..], &root).unwrap();
        let found = locate(&root, &github("lasse", "repo", None, None)).unwrap();
        assert_eq!(found.file_name().unwrap(), "lasse-repo-3f2a9c1");

        let nested = dir.join("nested");
        fs::create_dir(&nested).unwrap();
        unpack(
            &tarball(&[
                ("README.md", "hi"),
                ("plugins/timer/index.tsx", PLUGIN),
                ("plugins/notes/index.tsx", PLUGIN),
            ])[..],
            &nested,
        )
        .unwrap();
        let several = locate(&nested, &github("lasse", "repo", None, None)).unwrap_err();
        assert!(
            several.contains("plugins/notes, plugins/timer"),
            "{several}"
        );
        assert!(several.contains("github.com/lasse/repo/tree/HEAD/plugins/notes"));
        let one = locate(
            &nested,
            &github("lasse", "repo", None, Some("plugins/timer")),
        )
        .unwrap();
        assert!(one.ends_with("plugins/timer"));
        let missing = locate(&nested, &github("lasse", "repo", None, Some("nope"))).unwrap_err();
        assert!(missing.contains("no folder"));

        let empty = dir.join("empty");
        fs::create_dir(&empty).unwrap();
        unpack(&tarball(&[("README.md", "hi")])[..], &empty).unwrap();
        assert!(
            locate(&empty, &github("lasse", "repo", None, None))
                .unwrap_err()
                .contains("no Sidedoor plugin")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn installing_again_replaces_the_same_plugin_and_keeps_others() {
        let dir = scratch("replace");
        let plugins = dir.join("plugins");
        // Someone else's "timer" is already there.
        fs::create_dir_all(plugins.join("timer")).unwrap();
        fs::write(plugins.join("timer/index.tsx"), PLUGIN).unwrap();

        let source = github("lasse", "repo", None, None);
        let first = dir.join("first");
        fs::create_dir(&first).unwrap();
        unpack(&tarball(&[("index.tsx", PLUGIN)])[..], &first).unwrap();
        let installed = install(
            staged(&first, source.clone(), "a".repeat(40).as_str()),
            &plugins,
        )
        .unwrap();
        assert_eq!(installed.id, "timer-2");
        assert_eq!(installed.source.as_ref().unwrap().commit, "a".repeat(40));
        assert!(!first.exists(), "the download is cleaned up");

        let second = dir.join("second");
        fs::create_dir(&second).unwrap();
        unpack(
            &tarball(&[("index.tsx", PLUGIN), ("extra.ts", "export {}")])[..],
            &second,
        )
        .unwrap();
        let updated = install(staged(&second, source, "b".repeat(40).as_str()), &plugins).unwrap();
        assert_eq!(updated.id, "timer-2");
        assert_eq!(updated.source.unwrap().short_commit(), "bbbbbbb");
        assert!(plugins.join("timer-2/extra.ts").is_file());
        assert!(Source::read(&plugins.join("timer")).is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}

/// Downloads a real plugin; run with `cargo test -p plugin-host -- --ignored`.
#[cfg(test)]
#[test]
#[ignore = "needs the network"]
fn downloads_a_plugin_from_github() {
    let plugins = std::env::temp_dir().join(format!("sidedoor-github-{}", std::process::id()));
    let installer = GitHubInstaller {
        plugins: plugins.clone(),
        bun: None,
    };
    let source = GitHub::parse(
        "https://github.com/lassejlv/sidedoor/tree/main/sdk/examples/plugins/pomodoro",
    )
    .unwrap();
    let staged = installer.fetch(&source).unwrap();
    assert_eq!(staged.manifest.name, "Pomodoro");
    assert_eq!(staged.id, "pomodoro");
    let manifest = install(staged, &plugins).unwrap();
    assert_eq!(manifest.dir, plugins.join("pomodoro"));
    assert_eq!(manifest.source.unwrap().commit.len(), 40);
    let root = GitHub::parse("lassejlv/sidedoor").unwrap();
    assert!(
        installer
            .fetch(&root)
            .unwrap_err()
            .contains("no Sidedoor plugin")
    );
    let missing = GitHub::parse("lassejlv/does-not-exist-sidedoor").unwrap();
    assert!(
        installer
            .fetch(&missing)
            .unwrap_err()
            .contains("Couldn't find")
    );
    fs::remove_dir_all(plugins).unwrap();
}
