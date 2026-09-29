//! Images plugins name by URL. Each is downloaded once into the cache
//! folder, then drawn from there like any file, so a card never waits on
//! the network while it paints.

use crate::dock::Dock;
use gpui_kit::{App, AppContext as _, Entity, Global};
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    io::Read as _,
    path::PathBuf,
    time::{Duration, SystemTime},
};

/// Larger downloads are refused.
const MAX_BYTES: u64 = 10 * 1024 * 1024;
/// A cached copy older than this is fetched again, and drawn meanwhile.
const FRESH_FOR: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq)]
enum Fetch {
    Loading { stale: Option<PathBuf> },
    Ready(PathBuf),
    Failed,
}

#[derive(Default)]
struct RemoteImages(HashMap<String, Fetch>);

impl Global for RemoteImages {}

/// Whether `src` names a web image rather than a file.
pub fn is_remote(src: &str) -> bool {
    src.starts_with("https://") || src.starts_with("http://")
}

fn cached_path(url: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    crate::platform::cache_dir()
        .join("plugin-images")
        .join(format!("{:016x}", hasher.finish()))
}

fn fresh(path: &PathBuf) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < FRESH_FOR)
}

fn download(url: &str, to: &PathBuf) -> Result<(), String> {
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build()
        .get(url)
        .call()
        .map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("larger than 10 MB".into());
    }
    let dir = to.parent().ok_or("no cache folder")?;
    std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    // Written aside first, so a half-written file is never drawn.
    let partial = to.with_extension("partial");
    std::fs::write(&partial, bytes).map_err(|err| err.to_string())?;
    std::fs::rename(&partial, to).map_err(|err| err.to_string())
}

/// The file to draw for `url`: `None` until the first download finishes,
/// or if it failed. Starts the download on first sight, and redraws the
/// dock when it lands.
pub fn resolve(url: &str, dock: &Entity<Dock>, cx: &mut App) -> Option<PathBuf> {
    let known = cx
        .try_global::<RemoteImages>()
        .and_then(|images| images.0.get(url).cloned());
    match known {
        Some(Fetch::Ready(path)) => return Some(path),
        Some(Fetch::Loading { stale }) => return stale,
        Some(Fetch::Failed) => return None,
        None => {}
    }

    let path = cached_path(url);
    if fresh(&path) {
        cx.default_global::<RemoteImages>()
            .0
            .insert(url.to_string(), Fetch::Ready(path.clone()));
        return Some(path);
    }
    let stale = path.exists().then(|| path.clone());
    cx.default_global::<RemoteImages>().0.insert(
        url.to_string(),
        Fetch::Loading {
            stale: stale.clone(),
        },
    );

    let (url, dock) = (url.to_string(), dock.clone());
    let task = cx.background_spawn({
        let (url, path) = (url.clone(), path.clone());
        async move { download(&url, &path) }
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| {
            let fetch = match result {
                Ok(()) => Fetch::Ready(path),
                Err(err) => {
                    eprintln!("sidedoor: couldn't load image {url}: {err}");
                    match std::fs::metadata(&path) {
                        Ok(_) => Fetch::Ready(path),
                        Err(_) => Fetch::Failed,
                    }
                }
            };
            cx.default_global::<RemoteImages>().0.insert(url, fetch);
            dock.update(cx, |_, cx| cx.notify());
        });
    })
    .detach();
    stale
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write as _, net::TcpListener};

    /// Serves `body` once over plain HTTP; returns its URL.
    fn serve(status: &'static str, body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/image.png", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        url
    }

    #[test]
    fn downloads_land_whole_in_the_cache() {
        let dir = std::env::temp_dir().join(format!("sidedoor-images-{}", std::process::id()));
        let to = dir.join("image");
        download(&serve("200 OK", b"\x89PNG fake"), &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"\x89PNG fake");
        assert!(!to.with_extension("partial").exists());
        assert!(fresh(&to));

        let missing = download(&serve("404 Not Found", b"nope"), &dir.join("missing"));
        assert!(missing.is_err());
        assert!(!dir.join("missing").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_web_addresses_are_remote() {
        assert!(is_remote("https://example.com/a.png"));
        assert!(!is_remote("/Users/me/a.png"));
        assert_ne!(
            cached_path("https://a/1.png"),
            cached_path("https://a/2.png")
        );
    }
}
