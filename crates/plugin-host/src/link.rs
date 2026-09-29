//! The links a plugin can be installed from: a repository on GitHub, any
//! other Git remote (GitLab, Gitea, Bitbucket, a server of your own, SSH),
//! or a `.zip` or `.tar.gz` file.

use serde::{Deserialize, Serialize};

const HINT: &str = "Paste a link to a Git repository, like github.com/owner/repo, or to a \
                    .zip or .tar.gz file.";

/// Where a plugin's files come from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Origin {
    /// Downloaded through GitHub's API, so it needs no Git.
    GitHub { owner: String, repo: String },
    /// Any other repository, cloned with `git`.
    Git { git: String },
    /// A `.zip` or `.tar.gz` file.
    Archive { archive: String },
}

/// A plugin to install: where it is, which branch or tag, and the folder in
/// the repository that holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    #[serde(flatten)]
    pub origin: Origin,
    /// A branch, tag or commit; the default branch when missing.
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// The plugin's folder inside the repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Link {
    /// Reads what people paste: `owner/repo` for GitHub, a web link to a
    /// repository or a folder in one on any host, a clone URL (HTTPS or
    /// SSH), or a link to an archive.
    pub fn parse(input: &str) -> Result<Self, String> {
        let text = input.trim().trim_end_matches('/');
        if text.is_empty() || text.starts_with('-') || text.contains(char::is_whitespace) {
            return Err(HINT.into());
        }
        let without_query = text.split(['?', '#']).next().unwrap_or_default();

        if is_archive(without_query) {
            let url = if text.contains("://") {
                text.to_string()
            } else {
                format!("https://{text}")
            };
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(HINT.into());
            }
            return Ok(Self::at(Origin::Archive { archive: url }));
        }

        // `git@host:owner/repo.git`, as SSH clone URLs are usually written.
        if !text.contains("://")
            && let Some((user_host, path)) = text.split_once(':')
            && let Some((_, host)) = user_host.split_once('@')
        {
            if is_github(host) {
                return github(path);
            }
            check_path(path)?;
            return Ok(Self::at(Origin::Git {
                git: text.to_string(),
            }));
        }

        let (scheme, rest) = match text.split_once("://") {
            Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
            // A bare `owner/repo` has no dot before its first slash.
            None if !text.split('/').next().unwrap_or_default().contains('.') => {
                return github(without_query);
            }
            None => ("https".to_string(), text),
        };
        let (host, path) = rest.split_once('/').ok_or(HINT)?;
        let path = path.split(['?', '#']).next().unwrap_or_default();
        let bare_host = host.rsplit('@').next().unwrap_or(host);
        if is_github(bare_host) {
            return github(path);
        }
        match scheme.as_str() {
            "https" | "http" => web(&scheme, host, path),
            "ssh" | "git" => {
                check_path(path)?;
                Ok(Self::at(Origin::Git {
                    git: text.to_string(),
                }))
            }
            _ => Err(HINT.into()),
        }
    }

    fn at(origin: Origin) -> Self {
        Self {
            origin,
            reference: None,
            path: None,
        }
    }

    /// The service, for labels: "GitHub", "GitLab", "Git" or "Download".
    pub fn service(&self) -> &'static str {
        match &self.origin {
            Origin::GitHub { .. } => "GitHub",
            Origin::Git { git } => {
                let host = host_of(git).to_ascii_lowercase();
                if host.contains("gitlab") {
                    "GitLab"
                } else if host.contains("bitbucket") {
                    "Bitbucket"
                } else if host.contains("codeberg") {
                    "Codeberg"
                } else {
                    "Git"
                }
            }
            Origin::Archive { .. } => "Download",
        }
    }

    /// A short name for the plugin's place, e.g. `owner/repo/folder` or
    /// `git.example.com/team/widgets`.
    pub fn label(&self) -> String {
        let base = match &self.origin {
            Origin::GitHub { owner, repo } => format!("{owner}/{repo}"),
            Origin::Git { git } => format!("{}/{}", host_of(git), repo_path(git)),
            Origin::Archive { archive } => {
                return archive
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .to_string();
            }
        };
        match &self.path {
            Some(path) => format!("{base}/{path}"),
            None => base,
        }
    }

    /// The repository's web page, or the archive itself.
    pub fn web_url(&self) -> String {
        match &self.origin {
            Origin::GitHub { .. } => self.link_text(),
            Origin::Git { git } if git.starts_with("http") => {
                git.trim_end_matches(".git").to_string()
            }
            Origin::Git { git } => format!("https://{}/{}", host_of(git), repo_path(git)),
            Origin::Archive { archive } => archive.clone(),
        }
    }

    /// A link that reads back as this one, to show people or paste again.
    pub fn link_text(&self) -> String {
        let folder = |base: String, marker: &str| {
            if self.reference.is_none() && self.path.is_none() {
                return base;
            }
            let mut url = format!(
                "{base}{marker}{}",
                self.reference.as_deref().unwrap_or("HEAD")
            );
            if let Some(path) = &self.path {
                url.push('/');
                url.push_str(path);
            }
            url
        };
        match &self.origin {
            Origin::GitHub { owner, repo } => {
                folder(format!("https://github.com/{owner}/{repo}"), "/tree/")
            }
            // SSH URLs have nowhere to put a folder; the web form does.
            Origin::Git { git } if git.starts_with("http") => folder(
                git.trim_end_matches(".git")
                    .trim_end_matches('/')
                    .to_string(),
                "/-/tree/",
            ),
            Origin::Git { .. } => folder(self.web_url(), "/-/tree/"),
            Origin::Archive { archive } => archive.clone(),
        }
    }

    /// What a plugin at the top of this link is called, e.g. the repository.
    pub fn name(&self) -> String {
        match &self.origin {
            Origin::GitHub { repo, .. } => repo.clone(),
            Origin::Git { git } => repo_path(git).rsplit('/').next().unwrap_or("plugin").into(),
            Origin::Archive { archive } => {
                let file = archive
                    .split(['?', '#'])
                    .next()
                    .unwrap_or_default()
                    .rsplit('/')
                    .next()
                    .unwrap_or("plugin");
                [".tar.gz", ".tgz", ".zip"]
                    .iter()
                    .find_map(|extension| file.strip_suffix(extension))
                    .unwrap_or(file)
                    .to_string()
            }
        }
    }

    /// Whether both name the same plugin, on any branch.
    pub fn same_plugin(&self, other: &Self) -> bool {
        let key = |link: &Self| {
            match &link.origin {
                Origin::GitHub { owner, repo } => format!("github.com/{owner}/{repo}"),
                Origin::Git { git } => format!("{}/{}", host_of(git), repo_path(git)),
                Origin::Archive { archive } => archive.clone(),
            }
            .to_ascii_lowercase()
        };
        key(self) == key(other) && self.path == other.path
    }
}

fn is_github(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "github.com" || host == "www.github.com"
}

fn is_archive(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    [".zip", ".tar.gz", ".tgz"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn check_path(path: &str) -> Result<(), String> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() || !parts.iter().all(|part| name_ok(part)) {
        return Err(HINT.into());
    }
    Ok(())
}

/// A folder path from link segments, if it's safe to join.
fn folder(parts: &[&str]) -> Result<Option<String>, String> {
    if parts.iter().any(|part| *part == ".." || *part == ".") {
        return Err(HINT.into());
    }
    Ok((!parts.is_empty()).then(|| parts.join("/")))
}

/// `owner/repo` and what follows it on github.com.
fn github(path: &str) -> Result<Link, String> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let [owner, repo, more @ ..] = parts.as_slice() else {
        return Err(HINT.into());
    };
    let repo = repo.trim_end_matches(".git");
    if !name_ok(owner) || !name_ok(repo) {
        return Err(HINT.into());
    }
    let (reference, path) = match more {
        [] => (None, None),
        [kind @ ("tree" | "blob"), reference, path @ ..] => {
            (Some(*reference), folder(&file_to_folder(kind, path))?)
        }
        _ => return Err(HINT.into()),
    };
    Ok(Link {
        origin: Origin::GitHub {
            owner: (*owner).to_string(),
            repo: repo.to_string(),
        },
        reference: reference.filter(|r| *r != "HEAD").map(Into::into),
        path,
    })
}

/// A link to a file, like `…/blob/main/timer/index.tsx`, means its folder.
fn file_to_folder<'a>(kind: &str, path: &[&'a str]) -> Vec<&'a str> {
    let mut path = path.to_vec();
    if kind == "blob" && path.last().is_some_and(|last| last.contains('.')) {
        path.pop();
    }
    path
}

/// A web link on a host other than GitHub. The repository is everything
/// before the part that names a branch and folder, which each host writes
/// its own way: GitLab `/-/tree/main/x`, Gitea and Forgejo
/// `/src/branch/main/x`, Bitbucket `/src/main/x`, others `/tree/main/x`.
fn web(scheme: &str, host: &str, path: &str) -> Result<Link, String> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let mut found = None;
    for index in 1..parts.len() {
        let rest = &parts[index..];
        found = match rest {
            ["-", kind @ ("tree" | "blob"), reference, path @ ..] => {
                Some((index, *reference, file_to_folder(kind, path)))
            }
            ["src", "branch" | "tag" | "commit", reference, path @ ..] => {
                Some((index, *reference, path.to_vec()))
            }
            [kind @ ("tree" | "blob"), reference, path @ ..] => {
                Some((index, *reference, file_to_folder(kind, path)))
            }
            ["src", reference, path @ ..] => Some((index, *reference, path.to_vec())),
            _ => None,
        };
        if found.is_some() {
            break;
        }
    }
    let (repo, reference, path) = match found {
        Some((index, reference, path)) => (&parts[..index], Some(reference), folder(&path)?),
        None => (&parts[..], None, None),
    };
    if repo.is_empty() || !repo.iter().all(|part| name_ok(part)) {
        return Err(HINT.into());
    }
    Ok(Link {
        origin: Origin::Git {
            git: format!("{scheme}://{host}/{}", repo.join("/")),
        },
        reference: reference.filter(|r| *r != "HEAD").map(Into::into),
        path,
    })
}

/// The host of a clone URL, without user or port.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    // scp-like `git@host:path` splits at ':' before any '/'.
    let host = if url.contains("://") {
        rest.split('/').next().unwrap_or_default()
    } else {
        host
    };
    let host = host.rsplit('@').next().unwrap_or(host);
    host.split(':').next().unwrap_or(host)
}

/// The repository's path on its host, without `.git`.
fn repo_path(url: &str) -> String {
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map_or("", |(_, path)| path),
        None => url.split_once(':').map_or("", |(_, path)| path),
    };
    path.trim_matches('/').trim_end_matches(".git").to_string()
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

    fn git(url: &str, reference: Option<&str>, path: Option<&str>) -> Link {
        Link {
            origin: Origin::Git { git: url.into() },
            reference: reference.map(Into::into),
            path: path.map(Into::into),
        }
    }

    #[test]
    fn github_links_use_github() {
        let plain = github("lasse", "timer", None, None);
        for input in [
            "lasse/timer",
            "github.com/lasse/timer",
            "https://github.com/lasse/timer",
            "https://www.github.com/lasse/timer/",
            "https://github.com/lasse/timer.git",
            "git@github.com:lasse/timer.git",
            "ssh://git@github.com/lasse/timer.git",
            "  https://github.com/lasse/timer?tab=readme  ",
        ] {
            assert_eq!(Link::parse(input), Ok(plain.clone()), "{input}");
        }
        assert_eq!(
            Link::parse("https://github.com/lasse/widgets/tree/main/plugins/timer"),
            Ok(github(
                "lasse",
                "widgets",
                Some("main"),
                Some("plugins/timer")
            ))
        );
        assert_eq!(
            Link::parse("github.com/lasse/widgets/blob/v2/timer/index.tsx"),
            Ok(github("lasse", "widgets", Some("v2"), Some("timer")))
        );
        assert_eq!(
            Link::parse("github.com/lasse/widgets/tree/HEAD/timer"),
            Ok(github("lasse", "widgets", None, Some("timer")))
        );
    }

    #[test]
    fn other_hosts_clone_with_git() {
        assert_eq!(
            Link::parse("https://gitlab.com/team/sub/widgets"),
            Ok(git("https://gitlab.com/team/sub/widgets", None, None))
        );
        assert_eq!(
            Link::parse("gitlab.com/team/widgets/-/tree/main/plugins/timer"),
            Ok(git(
                "https://gitlab.com/team/widgets",
                Some("main"),
                Some("plugins/timer")
            ))
        );
        assert_eq!(
            Link::parse("https://codeberg.org/lasse/widgets/src/branch/dev/timer"),
            Ok(git(
                "https://codeberg.org/lasse/widgets",
                Some("dev"),
                Some("timer")
            ))
        );
        assert_eq!(
            Link::parse("https://bitbucket.org/lasse/widgets/src/main/timer/"),
            Ok(git(
                "https://bitbucket.org/lasse/widgets",
                Some("main"),
                Some("timer")
            ))
        );
        assert_eq!(
            Link::parse("https://git.example.com:8443/widgets.git"),
            Ok(git("https://git.example.com:8443/widgets.git", None, None))
        );
        assert_eq!(
            Link::parse("git@gitlab.example.com:team/widgets.git"),
            Ok(git("git@gitlab.example.com:team/widgets.git", None, None))
        );
        assert_eq!(
            Link::parse("ssh://git@git.example.com:2222/team/widgets.git"),
            Ok(git(
                "ssh://git@git.example.com:2222/team/widgets.git",
                None,
                None
            ))
        );
    }

    #[test]
    fn archives_download_as_files() {
        let link = Link::parse("https://example.com/files/timer-1.2.zip?download=1").unwrap();
        assert_eq!(
            link.origin,
            Origin::Archive {
                archive: "https://example.com/files/timer-1.2.zip?download=1".into()
            }
        );
        assert_eq!(link.name(), "timer-1.2");
        assert_eq!(
            Link::parse("example.com/timer.tar.gz").unwrap().origin,
            Origin::Archive {
                archive: "https://example.com/timer.tar.gz".into()
            }
        );
    }

    #[test]
    fn nonsense_is_refused() {
        for bad in [
            "",
            "lasse",
            "-upload-pack=touch /tmp/x",
            "file:///etc/passwd",
            "ext::sh -c touch% /tmp/pwned",
            "github.com/lasse/timer/issues",
            "github.com/lasse/timer/tree/main/../x",
            "https://gitlab.com/team/widgets/-/tree/main/../../x",
            "github.com/la sse/timer",
            "ftp://example.com/timer.zip",
        ] {
            assert!(Link::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn labels_and_links_read_back() {
        let gitlab = git(
            "https://gitlab.com/team/widgets",
            Some("main"),
            Some("timer"),
        );
        assert_eq!(gitlab.service(), "GitLab");
        assert_eq!(gitlab.label(), "gitlab.com/team/widgets/timer");
        assert_eq!(gitlab.name(), "widgets");
        assert_eq!(gitlab.web_url(), "https://gitlab.com/team/widgets");
        assert_eq!(Link::parse(&gitlab.link_text()), Ok(gitlab.clone()));

        let ssh = git("git@git.example.com:team/widgets.git", None, None);
        assert_eq!(ssh.service(), "Git");
        assert_eq!(ssh.label(), "git.example.com/team/widgets");
        assert_eq!(ssh.web_url(), "https://git.example.com/team/widgets");
        assert!(ssh.same_plugin(&git(
            "https://git.example.com/team/widgets",
            Some("dev"),
            None
        )));

        let hub = github("lasse", "widgets", None, Some("plugins/timer"));
        assert_eq!(
            hub.link_text(),
            "https://github.com/lasse/widgets/tree/HEAD/plugins/timer"
        );
        assert_eq!(Link::parse(&hub.link_text()), Ok(hub.clone()));
        assert!(hub.same_plugin(&github(
            "Lasse",
            "Widgets",
            Some("dev"),
            Some("plugins/timer")
        )));
        assert!(!hub.same_plugin(&github("lasse", "widgets", None, None)));
    }

    #[test]
    fn saved_links_read_back_including_older_github_ones() {
        let older: Link =
            serde_json::from_str(r#"{"owner":"lasse","repo":"timer","ref":"main"}"#).unwrap();
        assert_eq!(older, github("lasse", "timer", Some("main"), None));
        for link in [
            github("lasse", "timer", None, Some("x")),
            git("git@example.com:a/b.git", Some("v1"), None),
            Link::parse("https://example.com/t.zip").unwrap(),
        ] {
            let json = serde_json::to_string(&link).unwrap();
            assert_eq!(serde_json::from_str::<Link>(&json).unwrap(), link, "{json}");
        }
    }
}
