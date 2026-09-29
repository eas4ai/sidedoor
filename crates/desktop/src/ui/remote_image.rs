//! GPUI state for asynchronously cached plugin images.
use crate::app::dock::Dock;
use gpui_kit::{App, AppContext as _, Entity, Global};
pub use services::images::is_remote;
use services::images::{cached_path, download, fresh};
use std::{collections::HashMap, path::PathBuf};
#[derive(Clone, Debug, PartialEq)]
enum Fetch {
    Loading { stale: Option<PathBuf> },
    Ready(PathBuf),
    Failed,
}

#[derive(Default)]
struct RemoteImages(HashMap<String, Fetch>);

impl Global for RemoteImages {}

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
