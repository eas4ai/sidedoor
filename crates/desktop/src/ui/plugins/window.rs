//! A window a plugin opened with `sidedoor.openWindow(key)`: a native
//! titled window whose content the plugin draws, like its card.

use crate::app::dock::Dock;
use crate::ui::plugins::renderer::Surface;
use crate::ui::theme::{Palette, text};
use gpui_kit::{
    Context, Entity, EventEmitter, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    TestSupportExt as _, Window, div, prelude::FluentBuilder as _, px,
};

/// The height of the title bar strip the traffic lights sit in; the system
/// caption shows the title outside macOS.
pub const TITLE_BAR: f32 = crate::ui::chrome::title_strip(38.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginWindowEvent {
    /// The close button was pressed; the window is going away.
    Closed,
}

pub struct PluginWindow {
    dock: Entity<Dock>,
    /// Images this window draws, freed once it stops drawing them.
    images: Entity<crate::ui::image_cache::FrameImages>,
    plugin: SharedString,
    key: SharedString,
    title: SharedString,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PluginWindowEvent> for PluginWindow {}

impl PluginWindow {
    pub fn new(
        dock: Entity<Dock>,
        plugin: SharedString,
        key: SharedString,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let view = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            view.update(cx, |_, cx| cx.emit(PluginWindowEvent::Closed))
                .ok();
            true
        });
        crate::ui::theme::sync_kit_theme(window, cx);
        let subscriptions = vec![
            cx.observe(&dock, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, window, cx| {
                crate::ui::theme::sync_kit_theme(window, cx);
                cx.notify();
            }),
        ];
        Self {
            images: crate::ui::image_cache::FrameImages::new(cx),
            dock,
            plugin,
            key,
            title,
            _subscriptions: subscriptions,
        }
    }
}

impl PluginWindow {
    fn content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = self.dock.read(cx);
        let palette = Palette::new(window, dock.accessibility);
        let state = dock.plugin(&self.plugin);
        let tree = state.and_then(|state| state.windows.get(self.key.as_ref()).cloned());
        let problem = state.and_then(|state| state.problem.clone());

        let content = match (problem, tree) {
            (Some(problem), _) => vec![notice(problem, palette)],
            (None, None) => vec![notice("Starting…".into(), palette)],
            (None, Some(tree)) => {
                let surface = Surface {
                    dock: self.dock.clone(),
                    plugin: self.plugin.clone(),
                    palette,
                };
                surface.render(&tree, &format!("window:{}", self.key), window, cx)
            }
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.surface)
            .text_color(palette.label)
            .text_size(px(text::BODY))
            .when(crate::ui::chrome::INSET_TITLE_BAR, |this| {
                this.child(
                    div()
                        .h(px(TITLE_BAR))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child(self.title.clone()),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!(
                        "plugin-window:{}:{}",
                        self.plugin, self.key
                    )))
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .px(px(16.0))
                            .pb(px(16.0))
                            .when(!crate::ui::chrome::INSET_TITLE_BAR, |this| {
                                this.pt(px(16.0))
                            })
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .children(content),
                    ),
            )
    }
}

impl Render for PluginWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.images
            .update(cx, |images, cx| images.sweep(window, cx));
        gpui_kit::image_cache(self.images.clone())
            .size_full()
            .child(self.content(window, cx))
    }
}

fn notice(message: SharedString, palette: Palette) -> gpui_kit::AnyElement {
    div()
        .text_color(palette.secondary)
        .child(message)
        .into_any_element()
}
