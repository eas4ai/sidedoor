//! Installing plugins from a link: download a repository at one commit (or
//! an archive), find the plugin inside it, and move it into the plugins
//! folder. Nothing in it runs until the user adds it to the dock.
//!
//! GitHub downloads go through its API, so they need no Git. Other
//! repositories are cloned with `git`, which reaches any host the user's
//! Git can, with their credentials. Archives are plain downloads.

use crate::{Link, Manifest, Origin, sdk::slug};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Kept in an installed plugin's folder: where it came from, for updates.
pub const SOURCE_FILE: &str = ".sidedoor-source.json";
/// Downloads stop here; a plugin is a few source files.
const MAX_DOWNLOAD: u64 = 50 * 1024 * 1024;
/// How long one `git` command may take before it's stopped.
const GIT_TIMEOUT: Duration = Duration::from_secs(120);
const API: &str = "https://api.github.com";

/// Where an installed plugin came from, and which version of it this is:
/// the commit for a repository, a digest of the file for an archive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    #[serde(flatten)]
    pub link: Link,
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

    /// The version as Git hosts show commits, e.g. `3f2a9c1`.
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
    /// Downloads `link` at its newest version and gets it ready to install.
    fn fetch(&self, link: &Link) -> Result<Staged, String>;
    /// The newest version of `link`, to tell whether an update exists.
    fn latest_commit(&self, link: &Link) -> Result<String, String>;
}

/// Downloads into a folder beside the plugins, so installing is a rename.
pub struct Downloader {
    pub plugins: PathBuf,
    /// Installs a plugin's npm dependencies, when it has any.
    pub bun: Option<PathBuf>,
}

impl Installer for Downloader {
    fn fetch(&self, link: &Link) -> Result<Staged, String> {
        let root = staging_dir(&self.plugins).map_err(|err| err.to_string())?;
        // Removes the download if anything below fails.
        let cleanup = Cleanup(root.clone());
        let commit = match &link.origin {
            Origin::GitHub { owner, repo } => {
                let commit = self.latest_commit(link)?;
                let url = format!("{API}/repos/{owner}/{repo}/tarball/{commit}");
                let response = agent(&url)
                    .get(&url)
                    .call()
                    .map_err(|err| describe(err, link))?;
                unpack(response.into_reader(), &root)
                    .map_err(|err| format!("Couldn't unpack {}: {err}", link.label()))?;
                commit
            }
            Origin::Git { git } => clone(git, link, &root.join("repo"))?,
            Origin::Archive { archive } => {
                let bytes = download(archive, link)?;
                let lower = archive
                    .split(['?', '#'])
                    .next()
                    .unwrap_or_default()
                    .to_lowercase();
                if lower.ends_with(".zip") {
                    unzip(&bytes, &root)
                } else {
                    unpack(&bytes[..], &root)
                }
                .map_err(|err| format!("Couldn't unpack {}: {err}", link.name()))?;
                digest(&bytes)
            }
        };
        let (top, dir) = locate(&root, link)?;
        install_dependencies(&dir, self.bun.as_deref())?;
        let manifest = Manifest::read(&dir).ok_or("The plugin can't be read.")?;
        let id = slug(&if dir == top {
            link.name()
        } else {
            manifest.id.clone()
        });
        std::mem::forget(cleanup);
        Ok(Staged {
            manifest,
            source: Source {
                link: link.clone(),
                commit,
            },
            id,
            root,
        })
    }

    fn latest_commit(&self, link: &Link) -> Result<String, String> {
        match &link.origin {
            Origin::GitHub { owner, repo } => {
                let reference = link.reference.as_deref().unwrap_or("HEAD");
                let url = format!("{API}/repos/{owner}/{repo}/commits/{reference}");
                let sha = agent(&url)
                    .get(&url)
                    .set("Accept", "application/vnd.github.sha")
                    .call()
                    .map_err(|err| describe(err, link))?
                    .into_string()
                    .map_err(|err| format!("Couldn't read GitHub's answer: {err}"))?;
                let sha = sha.trim();
                if !is_commit(sha) {
                    return Err("GitHub didn't say which commit is newest.".into());
                }
                Ok(sha.to_string())
            }
            Origin::Git { git } => latest_remote_commit(git, link),
            // A file has no commits; its contents are its version.
            Origin::Archive { archive } => Ok(digest(&download(archive, link)?)),
        }
    }
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn is_commit(text: &str) -> bool {
    text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// An agent for requests to `url`, through the environment's proxy unless
/// `NO_PROXY` or a loopback address says to connect directly.
fn agent(url: &str) -> ureq::Agent {
    let builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .user_agent("Sidedoor");
    match platform::proxy::for_url(url).and_then(|proxy| ureq::Proxy::new(proxy).ok()) {
        Some(proxy) => builder.proxy(proxy),
        None => builder,
    }
    .build()
}

fn describe(err: ureq::Error, link: &Link) -> String {
    let service = match link.origin {
        Origin::GitHub { .. } => "GitHub",
        _ => "The server",
    };
    match err {
        ureq::Error::Status(404 | 422, _) => format!(
            "Couldn't find {}. Check the link, and that it's public.",
            link.label()
        ),
        ureq::Error::Status(403 | 429, _) if service == "GitHub" => {
            "GitHub is limiting downloads right now. Try again in a few minutes.".into()
        }
        ureq::Error::Status(401 | 403, _) => format!("{} needs a login to download.", link.label()),
        ureq::Error::Status(code, _) => format!("{service} answered with an error ({code})."),
        ureq::Error::Transport(err) => format!("Couldn't download {}: {err}", link.label()),
    }
}

/// Reads a whole download into memory, up to the size limit.
fn download(url: &str, link: &Link) -> Result<Vec<u8>, String> {
    let response = agent(url)
        .get(url)
        .call()
        .map_err(|err| describe(err, link))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("Couldn't download {}: {err}", link.label()))?;
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err("The download is larger than 50 MB.".into());
    }
    Ok(bytes)
}

/// A file's version: the SHA-256 of its bytes, in hex.
fn digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

/// Unpacks a zip file into `root`. Entries that would land outside it, and
/// links, are skipped.
pub(crate) fn unzip(bytes: &[u8], root: &Path) -> io::Result<()> {
    let mut archive = zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(io::Error::other)?;
    let mut written = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io::Error::other)?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let target = root.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if entry.is_symlink() {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(&target)?;
        written += io::copy(
            &mut (&mut entry).take(MAX_DOWNLOAD - written + 1),
            &mut file,
        )?;
        if written > MAX_DOWNLOAD {
            return Err(io::Error::other("the unpacked files are larger than 50 MB"));
        }
    }
    Ok(())
}

// MARK: Git

/// Git, where apps opened from Finder can find it. On a Mac without the
/// developer tools, `/usr/bin/git` only offers to install them, so it
/// counts only once they're there.
fn find_git() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SIDEDOOR_GIT") {
        return Some(PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    let candidates = {
        let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|paths| {
                std::env::split_paths(&paths)
                    .map(|dir| dir.join("git.exe"))
                    .collect()
            })
            .unwrap_or_default();
        for base in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(base) {
                candidates.push(PathBuf::from(&base).join("Git/cmd/git.exe"));
                candidates.push(PathBuf::from(base).join("Programs/Git/cmd/git.exe"));
            }
        }
        candidates
    };
    #[cfg(not(target_os = "windows"))]
    let candidates = {
        let mut candidates = vec![
            PathBuf::from("/opt/homebrew/bin/git"),
            PathBuf::from("/usr/local/bin/git"),
        ];
        let developer_tools = Command::new("/usr/bin/xcode-select")
            .arg("-p")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if developer_tools || !cfg!(target_os = "macos") {
            candidates.push(PathBuf::from("/usr/bin/git"));
        }
        candidates
    };
    candidates.into_iter().find(|path| path.is_file())
}

fn missing_git(link: &Link) -> String {
    let fix = if cfg!(target_os = "windows") {
        "Install Git from git-scm.com"
    } else {
        "Install Apple's command line tools by running `xcode-select --install` in Terminal"
    };
    format!(
        "Installing from {} needs Git. {fix}, then try again.",
        link.service()
    )
}

/// Runs `git` with `args` and returns what it printed. It never waits for a
/// password: a repository that needs one must already work in Terminal.
fn git(args: &[&str], link: &Link) -> Result<String, String> {
    let git = find_git().ok_or_else(|| missing_git(link))?;
    let mut command = Command::new(git);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Couldn't run Git: {err}"))?;
    // Read both pipes as they fill, so a chatty Git can't block.
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        })
    };
    let stdout = read(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    let stderr = read(child.stderr.take().map(|pipe| Box::new(pipe) as _));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > GIT_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} took too long to answer.", link.label()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(format!("Couldn't run Git: {err}")),
        }
    };
    let (stdout, stderr) = (
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default(),
    );
    if status.success() {
        Ok(stdout)
    } else {
        Err(explain_git(&stderr, link))
    }
}

/// Git's complaint in a sentence people can act on.
fn explain_git(stderr: &str, link: &Link) -> String {
    let lower = stderr.to_lowercase();
    let label = link.label();
    if lower.contains("could not resolve host") || lower.contains("could not connect") {
        format!("Couldn't reach {label}. Check the link and your connection.")
    } else if lower.contains("remote branch") && lower.contains("not found") {
        format!(
            "{label} has no branch or tag “{}”.",
            link.reference.as_deref().unwrap_or_default()
        )
    } else if lower.contains("authentication failed")
        || lower.contains("could not read username")
        || lower.contains("permission denied")
        || lower.contains("terminal prompts disabled")
        || lower.contains("host key verification failed")
    {
        // Hosts answer a missing repository like a private one.
        format!(
            "Couldn't open {label}. Check the link. If it's private, make sure \
             `git clone {}` works in Terminal, then try again.",
            link.web_url()
        )
    } else if lower.contains("not found")
        || lower.contains("does not appear to be a git repository")
        || lower.contains("does not exist")
    {
        format!("Couldn't find {label}. Check the link, and that you can see it.")
    } else {
        let reason = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Git failed")
            .trim()
            .trim_start_matches("fatal: ");
        format!("Couldn't download {label}: {reason}")
    }
}

/// The commit `link`'s branch or tag points at, without downloading it.
fn latest_remote_commit(url: &str, link: &Link) -> Result<String, String> {
    let reference = link.reference.as_deref().unwrap_or("HEAD");
    if is_commit(reference) {
        return Ok(reference.to_string());
    }
    let output = git(&["ls-remote", "--", url, reference], link)?;
    let refs: Vec<(&str, &str)> = output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    // A tag's own line points at the tag; `^{}` is the commit it names.
    refs.iter()
        .find(|(_, name)| name.ends_with("^{}"))
        .or_else(|| refs.first())
        .map(|(sha, _)| sha.to_string())
        .filter(|sha| is_commit(sha))
        .ok_or_else(|| format!("{} has no branch or tag “{reference}”.", link.label()))
}

/// Clones the newest commit of `link` into `into`, without its history, and
/// returns that commit.
fn clone(url: &str, link: &Link, into: &Path) -> Result<String, String> {
    let into_text = into.to_string_lossy();
    match link.reference.as_deref() {
        Some(commit) if is_commit(commit) => {
            git(&["init", "--quiet", &into_text], link)?;
            git(
                &[
                    "-C", &into_text, "fetch", "--quiet", "--depth", "1", "--", url, commit,
                ],
                link,
            )?;
            git(
                &["-C", &into_text, "checkout", "--quiet", "FETCH_HEAD"],
                link,
            )?;
        }
        Some(reference) => {
            git(
                &[
                    "clone",
                    "--quiet",
                    "--depth",
                    "1",
                    "--single-branch",
                    "--branch",
                    reference,
                    "--",
                    url,
                    &into_text,
                ],
                link,
            )?;
        }
        None => {
            git(
                &["clone", "--quiet", "--depth", "1", "--", url, &into_text],
                link,
            )?;
        }
    }
    let commit = git(&["-C", &into_text, "rev-parse", "HEAD"], link)?
        .trim()
        .to_string();
    // The plugin is its files; Git's own folder only takes space.
    let _ = fs::remove_dir_all(into.join(".git"));
    if !is_commit(&commit) {
        return Err(format!(
            "Git didn't say which commit {} is at.",
            link.label()
        ));
    }
    Ok(commit)
}

// MARK: Finding the plugin

fn is_plugin_dir(entry: &fs::DirEntry) -> bool {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    entry.file_type().is_ok_and(|kind| kind.is_dir())
        && !name.starts_with('.')
        && name != "node_modules"
        && name != "__MACOSX"
}

/// The top of an unpacked download, and the plugin folder in it. A download
/// that holds just one folder, as repository archives and clones do, starts
/// in that folder.
pub(crate) fn locate(root: &Path, link: &Link) -> Result<(PathBuf, PathBuf), String> {
    let entries: Vec<fs::DirEntry> = fs::read_dir(root)
        .map_err(|err| err.to_string())?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && name != "__MACOSX"
        })
        .collect();
    let top = match entries.as_slice() {
        [] => return Err("The download was empty.".into()),
        [only] if is_plugin_dir(only) => only.path(),
        _ => root.to_path_buf(),
    };
    let base = match &link.path {
        Some(path) => top.join(path),
        None => top.clone(),
    };
    if !base.is_dir() {
        return Err(format!(
            "{} has no folder “{}”.",
            Link {
                path: None,
                ..link.clone()
            }
            .label(),
            link.path.as_deref().unwrap_or_default()
        ));
    }
    if Manifest::read(&base).is_some() {
        return Ok((top, base));
    }
    // Look a couple of levels down, as in a repository of several plugins.
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
            link.label()
        )),
        [only] => Ok((top, only.clone())),
        several => {
            let relative = |path: &PathBuf| {
                path.strip_prefix(&top)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/")
            };
            let names: Vec<String> = several.iter().map(relative).collect();
            let choose = match &link.origin {
                Origin::Archive { .. } => "Download one of them on its own.".to_string(),
                _ => {
                    let example = Link {
                        path: Some(names[0].clone()),
                        ..link.clone()
                    };
                    format!(
                        "Paste the link to the one you want, like {}.",
                        example.link_text().trim_start_matches("https://")
                    )
                }
            };
            Err(format!(
                "{} has several plugins: {}. {choose}",
                link.label(),
                names.join(", "),
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
        if Source::read(&target).is_some_and(|old| old.link.same_plugin(&staged.source.link)) {
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

    fn github(owner: &str, repo: &str, reference: Option<&str>, path: Option<&str>) -> Link {
        Link {
            origin: Origin::GitHub {
                owner: owner.into(),
                repo: repo.into(),
            },
            reference: reference.map(Into::into),
            path: path.map(Into::into),
        }
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

    fn staged(root: &Path, source: Link, commit: &str) -> Staged {
        let (_, dir) = locate(root, &source).unwrap();
        Staged {
            manifest: Manifest::read(&dir).unwrap(),
            source: Source {
                link: source,
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
        let (top, found) = locate(&root, &github("lasse", "repo", None, None)).unwrap();
        assert_eq!(found.file_name().unwrap(), "lasse-repo-3f2a9c1");
        assert_eq!(top, found);

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
        .unwrap()
        .1;
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

/// Downloads real plugins; run with `cargo test -p plugin-host -- --ignored`.
#[cfg(test)]
#[test]
#[ignore = "needs the network"]
fn downloads_plugins_from_github_git_and_archives() {
    let plugins = std::env::temp_dir().join(format!("sidedoor-download-{}", std::process::id()));
    let installer = Downloader {
        plugins: plugins.clone(),
        bun: None,
    };
    let fetch = |link: &str| installer.fetch(&Link::parse(link).unwrap());

    let staged = fetch("https://github.com/lassejlv/sidedoor/tree/main/plugins/pomodoro").unwrap();
    assert_eq!(
        (staged.manifest.name.as_str(), staged.id.as_str()),
        ("Pomodoro", "pomodoro")
    );
    let manifest = install(staged, &plugins).unwrap();
    assert_eq!(manifest.dir, plugins.join("pomodoro"));
    assert_eq!(manifest.source.unwrap().commit.len(), 40);
    assert!(
        fetch("lassejlv/sidedoor")
            .unwrap_err()
            .contains("no Sidedoor plugin")
    );
    assert!(
        fetch("lassejlv/does-not-exist-sidedoor")
            .unwrap_err()
            .contains("Couldn't find")
    );

    // The same repository through Git, as any other host would be.
    let link = Link {
        origin: Origin::Git {
            git: "https://github.com/lassejlv/sidedoor-dice.git".into(),
        },
        reference: None,
        path: None,
    };
    let staged = installer.fetch(&link).unwrap();
    assert_eq!(
        (staged.manifest.name.as_str(), staged.id.as_str()),
        ("Dice", "sidedoor-dice")
    );
    assert_eq!(
        installer.latest_commit(&link).unwrap(),
        staged.source.commit
    );
    assert!(!staged.manifest.dir.join(".git").exists());
    let tagged = Link {
        reference: Some("no-such-branch".into()),
        ..link.clone()
    };
    assert!(
        installer
            .fetch(&tagged)
            .unwrap_err()
            .contains("no branch or tag")
    );

    // GitLab, cloned with Git.
    let gitlab = Link::parse("https://gitlab.com/gitlab-org/gitlab-test").unwrap();
    assert_eq!(gitlab.service(), "GitLab");
    assert!(
        installer
            .fetch(&gitlab)
            .unwrap_err()
            .contains("no Sidedoor plugin")
    );
    let missing = Link::parse("gitlab.com/lassejlv/no-such-repo-sidedoor").unwrap();
    assert!(
        installer
            .fetch(&missing)
            .unwrap_err()
            .contains("Couldn't open")
    );

    // GitHub serves every repository as an archive too.
    let staged =
        fetch("https://github.com/lassejlv/sidedoor-dice/archive/refs/heads/main.zip").unwrap();
    assert_eq!(staged.manifest.name, "Dice");
    assert_eq!(staged.source.commit.len(), 64);
    drop(staged);
    fs::remove_dir_all(plugins).unwrap();
}
