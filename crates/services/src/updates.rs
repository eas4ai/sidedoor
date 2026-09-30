//! Stable GitHub releases, verified complete payloads, and installation ownership.
//! Blocking work belongs on a background executor. No installed file changes
//! until the user asks to restart and the separate helper sees the app exit.
mod archive;
mod install;
#[cfg(test)]
mod tests;

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::TempDir;

const API: &str = "https://api.github.com/repos/lassejlv/sidedoor/releases/latest";
const REPO: &str = "https://github.com/lassejlv/sidedoor/releases/";
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
const MAX_METADATA: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    MacBundle,
    WindowsMsi,
    WindowsSetup,
    Debian,
    Rpm,
    Portable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    pub owner: Owner,
    pub os: String,
    pub arch: String,
    pub root: PathBuf,
    pub executable: PathBuf,
}

impl Installation {
    pub fn current() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|_| "Couldn't locate Sidedoor.")?;
        let parent = exe.parent().ok_or("Couldn't locate the app folder.")?;
        let os = std::env::consts::OS;
        let arch = match std::env::consts::ARCH {
            "aarch64" if os == "linux" => "aarch64",
            "aarch64" => "arm64",
            "x86_64" if os == "linux" => "x86_64",
            "x86_64" => "x64",
            _ => return Err("No updates are published for this architecture.".into()),
        };
        let (owner, root) = if os == "macos" {
            let root = parent
                .parent()
                .and_then(Path::parent)
                .ok_or("Not an installed app bundle.")?;
            if parent.file_name().and_then(|n| n.to_str()) != Some("MacOS")
                || root.extension().and_then(|n| n.to_str()) != Some("app")
            {
                return Err("Install Sidedoor.app to receive updates.".into());
            }
            (Owner::MacBundle, root.to_path_buf())
        } else {
            if !parent.join("resources/sdk/package.json").is_file()
                || !parent.join(bun_name(os)).is_file()
            {
                return Err(
                    "Updates are available in packaged installations. Download a release first."
                        .into(),
                );
            }
            let owner = if os == "windows" && parent.join(".sidedoor-bundle").is_file() {
                Owner::WindowsSetup
            } else if os == "windows" && parent.join(".sidedoor-msi").is_file() {
                Owner::WindowsMsi
            } else if os == "linux" && parent == Path::new("/usr/lib/sidedoor") {
                if owned_by(
                    "dpkg-query",
                    &["-S", "/usr/lib/sidedoor/sidedoor"],
                    "sidedoor:",
                ) {
                    Owner::Debian
                } else if owned_by(
                    "rpm",
                    &["-qf", "--qf", "%{NAME}", "/usr/lib/sidedoor/sidedoor"],
                    "sidedoor",
                ) {
                    Owner::Rpm
                } else {
                    return Err(
                        "Couldn't identify the package manager for this installation.".into(),
                    );
                }
            } else {
                Owner::Portable
            };
            (owner, parent.to_path_buf())
        };
        Ok(Self {
            owner,
            root,
            executable: exe,
            os: os.into(),
            arch: arch.into(),
        })
    }

    pub fn asset_name(&self) -> String {
        let suffix = match self.owner {
            Owner::MacBundle => "zip",
            Owner::WindowsMsi => "msi",
            Owner::WindowsSetup => "setup.exe",
            Owner::Debian => "deb",
            Owner::Rpm => "rpm",
            Owner::Portable if self.os == "windows" => "zip",
            Owner::Portable => "tar.gz",
        };
        if self.owner == Owner::WindowsSetup {
            format!("Sidedoor-windows-{}-{suffix}", self.arch)
        } else {
            format!("Sidedoor-{}-{}.{suffix}", self.os, self.arch)
        }
    }

    pub fn needs_admin(&self) -> bool {
        matches!(
            self.owner,
            Owner::WindowsMsi | Owner::WindowsSetup | Owner::Debian | Owner::Rpm
        )
    }
}

fn owned_by(command: &str, args: &[&str], prefix: &str) -> bool {
    Command::new(command)
        .args(args)
        .output()
        .ok()
        .is_some_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .starts_with(prefix)
        })
}

fn bun_name(os: &str) -> &'static str {
    if os == "windows" { "bun.exe" } else { "bun" }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub installation: Installation,
    pub asset: ManifestAsset,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestAsset {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    version: String,
    assets: Vec<ManifestAsset>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

pub struct Prepared {
    pub release: Release,
    pub payload: PathBuf,
    pub staging: TempDir,
}

pub trait Backend: Send + Sync {
    fn check(&self, current: &str) -> Result<Option<Release>, String>;
    fn prepare(&self, release: &Release) -> Result<Prepared, String>;
    /// Starts a detached helper. The caller must quit only after success.
    fn launch(&self, update: Prepared) -> Result<(), String>;
}

pub struct Github;

impl Backend for Github {
    fn check(&self, current: &str) -> Result<Option<Release>, String> {
        let response = match request(API) {
            Ok(response) => response,
            Err(error) if error == "HTTP 404" => return Ok(None),
            Err(error) => return Err(error),
        };
        let release: GithubRelease = json(response, MAX_METADATA)?;
        let candidate = Version::parse(release.tag_name.trim_start_matches('v'))
            .map_err(|_| "The release version couldn't be read.")?;
        if release.draft
            || release.prerelease
            || !candidate.pre.is_empty()
            || !newer(&release.tag_name, current)?
        {
            return Ok(None);
        }
        let installation = Installation::current()?;
        let metadata = release
            .assets
            .iter()
            .find(|a| a.name == "sidedoor-update.json")
            .ok_or("This release has no update manifest. Download it from GitHub Releases.")?;
        validate_asset_url(
            &metadata.browser_download_url,
            &release.tag_name,
            &metadata.name,
        )?;
        let manifest: Manifest = json(request(&metadata.browser_download_url)?, MAX_METADATA)?;
        select(release, manifest, installation).map(Some)
    }

    fn prepare(&self, release: &Release) -> Result<Prepared, String> {
        validate_digest(&release.asset.sha256)?;
        let cache = platform::cache_dir().join("updates");
        fs::create_dir_all(&cache).map_err(|_| "Couldn't create the update cache.")?;
        let staging = tempfile::Builder::new()
            .prefix("staged-")
            .tempdir_in(cache)
            .map_err(|e| e.to_string())?;
        let package = staging.path().join(&release.asset.name);
        download(release, &package)?;
        let payload = match release.installation.owner {
            Owner::MacBundle | Owner::Portable => {
                archive::extract(&package, staging.path(), &release.installation)?
            }
            _ => package,
        };
        if release.installation.owner == Owner::MacBundle {
            let status = Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&payload)
                .output()
                .map_err(|_| "Couldn't verify the app signature.")?;
            if !status.status.success() {
                return Err("The downloaded app's signature is invalid.".into());
            }
            let info = Command::new("/usr/libexec/PlistBuddy")
                .args(["-c", "Print :CFBundleIdentifier"])
                .arg(payload.join("Contents/Info.plist"))
                .output()
                .map_err(|e| e.to_string())?;
            if !info.status.success()
                || String::from_utf8_lossy(&info.stdout).trim() != "com.lassevestergaard.sidedoor"
            {
                return Err("The downloaded app has the wrong identity.".into());
            }
            let info = Command::new("/usr/libexec/PlistBuddy")
                .args(["-c", "Print :CFBundleShortVersionString"])
                .arg(payload.join("Contents/Info.plist"))
                .output()
                .map_err(|e| e.to_string())?;
            if !info.status.success()
                || String::from_utf8_lossy(&info.stdout).trim()
                    != release.version.split(['-', '+']).next().unwrap_or_default()
            {
                return Err("The downloaded app version doesn't match the release.".into());
            }
            let status = Command::new("lipo")
                .args(["-verify_arch", &release.installation.arch])
                .arg(payload.join("Contents/MacOS/sidedoor"))
                .output()
                .map_err(|e| e.to_string())?;
            if !status.status.success() {
                return Err("The downloaded app has the wrong CPU architecture.".into());
            }
        }
        Ok(Prepared {
            release: release.clone(),
            payload,
            staging,
        })
    }

    fn launch(&self, update: Prepared) -> Result<(), String> {
        install::launch(update)
    }
}

pub fn newer(candidate: &str, current: &str) -> Result<bool, String> {
    let version = |text: &str| {
        Version::parse(text.strip_prefix('v').unwrap_or(text))
            .map_err(|_| "The release version couldn't be read.".to_string())
    };
    Ok(version(candidate)? > version(current)?)
}

fn select(
    release: GithubRelease,
    manifest: Manifest,
    installation: Installation,
) -> Result<Release, String> {
    if manifest.schema_version != 1 || manifest.version != release.tag_name.trim_start_matches('v')
    {
        return Err("The update manifest doesn't match this release.".into());
    }
    let name = installation.asset_name();
    let mut assets = manifest
        .assets
        .into_iter()
        .filter(|asset| asset.name == name);
    let asset = assets
        .next()
        .ok_or("No update is available for this installation type and architecture.")?;
    if assets.next().is_some() {
        return Err("Duplicate update payload in the manifest.".into());
    }
    validate_digest(&asset.sha256)?;
    if asset.size == 0 || asset.size > MAX_DOWNLOAD {
        return Err("The update payload has an invalid size.".into());
    }
    let downloads: Vec<_> = release
        .assets
        .iter()
        .filter(|download| download.name == name)
        .collect();
    let [download] = downloads.as_slice() else {
        return Err("The release payload is missing or duplicated.".into());
    };
    if download.size != asset.size {
        return Err("The release payload size doesn't match the manifest.".into());
    }
    validate_asset_url(&download.browser_download_url, &release.tag_name, &name)?;
    Ok(Release {
        version: manifest.version,
        notes: release.body.unwrap_or_default(),
        installation,
        asset,
        url: download.browser_download_url.clone(),
    })
}

fn validate_digest(digest: &str) -> Result<(), String> {
    if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("The update checksum is missing or invalid.".into())
    }
}

fn validate_asset_url(url: &str, tag: &str, name: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| "Invalid release download URL.")?;
    if parsed.as_str() != format!("{REPO}download/{tag}/{name}") {
        return Err("The update download doesn't belong to this release.".into());
    }
    Ok(())
}

fn request(url: &str) -> Result<ureq::Response, String> {
    let mut url = url::Url::parse(url).map_err(|_| "Invalid update URL.")?;
    for _ in 0..6 {
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || !matches!(
                url.host_str(),
                Some(
                    "api.github.com"
                        | "github.com"
                        | "release-assets.githubusercontent.com"
                        | "objects.githubusercontent.com"
                )
            )
        {
            return Err("An update download redirected outside GitHub's HTTPS hosts.".into());
        }
        let response = crate::http::agent(
            url.as_str(),
            ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(180))
                .redirects(0),
        )
        .get(url.as_str())
        .set("User-Agent", "Sidedoor-Updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(code, _) => format!("HTTP {code}"),
            _ => "Couldn't reach GitHub. Check your connection and try again.".into(),
        })?;
        if (300..400).contains(&response.status()) {
            url = url
                .join(
                    response
                        .header("Location")
                        .ok_or("Missing update redirect.")?,
                )
                .map_err(|_| "Invalid update redirect.")?;
        } else {
            return Ok(response);
        }
    }
    Err("Too many update redirects.".into())
}

fn json<T: serde::de::DeserializeOwned>(response: ureq::Response, limit: u64) -> Result<T, String> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Couldn't read update metadata.")?;
    if bytes.len() as u64 > limit {
        return Err("Update metadata is too large.".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "The update metadata couldn't be read.".into())
}

fn download(release: &Release, to: &Path) -> Result<(), String> {
    let response = request(&release.url)?;
    copy_verified(response.into_reader(), to, &release.asset)
}

fn copy_verified(
    input: impl std::io::Read,
    to: &Path,
    asset: &ManifestAsset,
) -> Result<(), String> {
    let mut input = input.take(asset.size + 1);
    let mut output = File::create(to).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut count = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = input
            .read(&mut buffer)
            .map_err(|_| "The update download was interrupted.")?;
        if size == 0 {
            break;
        }
        output
            .write_all(&buffer[..size])
            .map_err(|_| "Couldn't save the update. Check free disk space.")?;
        digest.update(&buffer[..size]);
        count += size as u64;
    }
    output.sync_all().map_err(|e| e.to_string())?;
    if count != asset.size || format!("{:x}", digest.finalize()) != asset.sha256.to_lowercase() {
        return Err("Update verification failed. Nothing was installed; try again.".into());
    }
    Ok(())
}
