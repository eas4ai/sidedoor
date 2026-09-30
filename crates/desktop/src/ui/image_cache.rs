//! Keeps decoded images only while a window draws them.
//!
//! GPUI's default image cache keeps every image it ever decoded, in memory
//! and in the window's texture atlas. A plugin that shows album art or a
//! clipboard history that rolls over then grows without bound. A window
//! that wraps its content in [`FrameImages`] frees each image the first
//! frame it isn't drawn.
use gpui_kit::{
    App, AppContext as _, Entity, ImageCache, ImageCacheError, ImageCacheItem, RenderImage,
    Resource, Window,
};
use std::{collections::HashMap, sync::Arc};

pub struct FrameImages {
    entries: HashMap<Resource, Entry>,
}

struct Entry {
    item: ImageCacheItem,
    /// Drawn since the last sweep.
    used: bool,
}

impl FrameImages {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let images = cx.new(|_| Self {
            entries: HashMap::new(),
        });
        cx.observe_release(&images, |images, cx| {
            for (_, entry) in images.entries.drain() {
                if let Some(Ok(image)) = entry.item.get() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        images
    }

    /// Frees the images the last frame didn't draw. Call it at the start of
    /// the window's render, before its images load again.
    pub fn sweep(&mut self, window: &mut Window, cx: &mut App) {
        self.entries.retain(|_, entry| {
            if std::mem::take(&mut entry.used) {
                return true;
            }
            if let Some(Ok(image)) = entry.item.get() {
                cx.drop_image(image, Some(&mut *window));
            }
            false
        });
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl ImageCache for FrameImages {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let entry = self
            .entries
            .entry(resource.clone())
            .or_insert_with(|| Entry {
                item: ImageCacheItem::new(resource, cx),
                used: false,
            });
        entry.used = true;
        entry.item.use_image(window)
    }
}
