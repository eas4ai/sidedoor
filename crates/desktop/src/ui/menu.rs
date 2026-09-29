//! Context menus. macOS and Windows show the system menu; Linux has no
//! system popup menu an app can call, so it gets one drawn like `NSMenu`
//! in its own pop-up window, free to extend past the small dock window.

use crate::app::dock::Dock;
use gpui_kit::{Action, App, Entity, Pixels, Point, SharedString, Window};

enum Entry {
    Separator,
    Item {
        label: SharedString,
        action: Box<dyn Action>,
    },
}

/// A context menu, built like `NativeMenu` and shown at a point.
#[derive(Default)]
pub struct ContextMenu {
    entries: Vec<Entry>,
}

impl ContextMenu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn menu(mut self, label: impl Into<SharedString>, action: Box<dyn Action>) -> Self {
        self.entries.push(Entry::Item {
            label: label.into(),
            action,
        });
        self
    }

    pub fn separator(mut self) -> Self {
        if !matches!(self.entries.last(), None | Some(Entry::Separator)) {
            self.entries.push(Entry::Separator);
        }
        self
    }

    /// Shows the menu at `position`, in `window`'s coordinates. Choosing an
    /// item dispatches its action in `window`.
    pub fn show(
        self,
        position: Point<Pixels>,
        dock: &Entity<Dock>,
        window: &mut Window,
        cx: &mut App,
    ) {
        #[cfg(target_os = "linux")]
        drawn::show(self.entries, position, dock, window, cx);
        #[cfg(not(target_os = "linux"))]
        {
            let _ = dock;
            let mut menu = gpui_kit::component::native_menu::NativeMenu::new();
            for entry in self.entries {
                menu = match entry {
                    Entry::Separator => menu.separator(),
                    Entry::Item { label, action } => menu.menu(label, action),
                };
            }
            menu.show(position, window, cx);
        }
    }
}

#[cfg(target_os = "linux")]
mod drawn {
    use super::Entry;
    use crate::app::dock::Dock;
    use crate::ui::theme::{Palette, text};
    use gpui_kit::{
        AnyWindowHandle, App, AppContext as _, Bounds, BoxShadow, Context, Entity, FocusHandle,
        FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
        MouseDownEvent, ParentElement as _, Pixels, Point, Refineable as _, Render,
        StatefulInteractiveElement as _, StyleRefinement, Styled as _, TextRun, Window,
        WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, base::Root, canvas,
        div, font, point, prelude::FluentBuilder as _, px, size, transparent_black,
    };

    /// `NSMenu` metrics, in points.
    const RADIUS: f32 = 6.0;
    const PADDING: f32 = 5.0;
    const ITEM_HEIGHT: f32 = 22.0;
    const SEPARATOR_HEIGHT: f32 = 11.0;
    const HIGHLIGHT_RADIUS: f32 = 4.0;
    /// From the highlight's edge to the text, leaving room for a check mark.
    const TEXT_INSET: f32 = 15.0;
    const TEXT_END: f32 = 20.0;
    const MIN_WIDTH: f32 = 160.0;
    /// Room around the menu for its shadow.
    const SHADOW: f32 = 14.0;

    pub(super) fn show(
        entries: Vec<Entry>,
        position: Point<Pixels>,
        dock: &Entity<Dock>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if entries.is_empty() {
            return;
        }
        let origin = window.window_handle();
        let family = crate::ui::theme::ui_font(cx);
        let widest = entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { label, .. } => Some(label),
                Entry::Separator => None,
            })
            .map(|label| {
                let run = TextRun {
                    len: label.len(),
                    font: gpui_kit::Font {
                        weight: FontWeight::NORMAL,
                        ..font(family.clone())
                    },
                    color: gpui_kit::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                f32::from(
                    window
                        .text_system()
                        .layout_line(label, px(text::BODY), &[run], None)
                        .width,
                )
            })
            .fold(0.0, f32::max);
        let width = (PADDING * 2.0 + TEXT_INSET + widest + TEXT_END)
            .max(MIN_WIDTH)
            .ceil();
        let height = PADDING * 2.0
            + entries
                .iter()
                .map(|entry| match entry {
                    Entry::Item { .. } => ITEM_HEIGHT,
                    Entry::Separator => SEPARATOR_HEIGHT,
                })
                .sum::<f32>();

        // Open to the right of the pointer, or to its left when that would
        // leave the screen, as NSMenu does; likewise below or above.
        let pointer = window.bounds().origin + position;
        let screen = window
            .display(cx)
            .map(|display| display.bounds())
            .unwrap_or(window.bounds());
        let right = screen.origin.x + screen.size.width;
        let bottom = screen.origin.y + screen.size.height;
        let x = if pointer.x + px(width) > right {
            pointer.x - px(width)
        } else {
            pointer.x
        };
        let y = if pointer.y + px(height) > bottom {
            pointer.y - px(height)
        } else {
            pointer.y
        };
        let bounds = Bounds {
            origin: point(x - px(SHADOW), y - px(SHADOW)),
            size: size(px(width + SHADOW * 2.0), px(height + SHADOW * 2.0)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        };
        dock.update(cx, |dock, _| dock.set_menu_open(true));
        let dock = dock.clone();
        let opened = gpui_kit::open_window(options, cx, |_, cx| {
            cx.new(|cx| MenuView {
                entries,
                highlighted: None,
                focus: cx.focus_handle(),
                origin,
                dock: dock.clone(),
                done: false,
                native: None,
                _watch: None,
            })
        });
        let Ok((handle, view)) = opened else {
            dock.update(cx, |dock, _| dock.set_menu_open(false));
            return;
        };
        handle
            .update(cx, |_, window, cx| {
                Root::update(window, cx, |root, _, _| {
                    root.style()
                        .refine(&StyleRefinement::default().bg(transparent_black()));
                });
                let focus = view.read(cx).focus.clone();
                window.focus(&focus, cx);
                let Some(native) = ::platform::native::window_handle(window) else {
                    return;
                };
                ::platform::native::begin_menu(&native);
                // Close on a click anywhere else, as NSMenu does.
                let weak = view.downgrade();
                let watched = native.clone();
                let watch = cx.spawn(async move |cx| {
                    loop {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(16))
                            .await;
                        if !::platform::native::pressed_outside(&watched) {
                            continue;
                        }
                        let _ = handle.update(cx, |_, window, cx| {
                            if let Some(view) = weak.upgrade() {
                                view.update(cx, |this, cx| this.close(None, window, cx));
                            }
                        });
                        break;
                    }
                });
                view.update(cx, |this, _| {
                    this.native = Some(native);
                    this._watch = Some(watch);
                });
            })
            .ok();
    }

    struct MenuView {
        entries: Vec<Entry>,
        highlighted: Option<usize>,
        focus: FocusHandle,
        /// The window the menu was opened from, where its action runs.
        origin: AnyWindowHandle,
        dock: Entity<Dock>,
        done: bool,
        native: Option<::platform::native::NativeWindow>,
        _watch: Option<gpui_kit::Task<()>>,
    }

    impl MenuView {
        fn close(&mut self, choice: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
            if std::mem::replace(&mut self.done, true) {
                return;
            }
            if let Some(native) = self.native.take() {
                ::platform::native::end_menu(&native);
            }
            let action = choice.and_then(|index| match &self.entries[index] {
                Entry::Item { action, .. } => Some(action.boxed_clone()),
                Entry::Separator => None,
            });
            window.remove_window();
            self.dock.update(cx, |dock, _| dock.set_menu_open(false));
            let origin = self.origin;
            // After this window is gone, as a native menu's action runs
            // after the menu closes.
            cx.defer(move |cx| {
                if let Some(action) = action {
                    origin
                        .update(cx, |_, window, cx| window.dispatch_action(action, cx))
                        .ok();
                }
            });
        }

        fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
            let items: Vec<usize> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| matches!(entry, Entry::Item { .. }))
                .map(|(index, _)| index)
                .collect();
            if items.is_empty() {
                return;
            }
            let position = self
                .highlighted
                .and_then(|current| items.iter().position(|&index| index == current));
            let next = match (position, delta > 0) {
                (None, true) => 0,
                (None, false) => items.len() - 1,
                (Some(at), true) => (at + 1).min(items.len() - 1),
                (Some(at), false) => at.saturating_sub(1),
            };
            self.highlighted = Some(items[next]);
            cx.notify();
        }

        fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
            match event.keystroke.key.as_str() {
                "escape" => self.close(None, window, cx),
                "down" => self.step(1, cx),
                "up" => self.step(-1, cx),
                "enter" | "space" => {
                    if let Some(index) = self.highlighted {
                        self.close(Some(index), window, cx);
                    }
                }
                _ => {}
            }
        }
    }

    impl Render for MenuView {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let accessibility = self.dock.read(cx).accessibility;
            let palette = Palette::new(window, accessibility);
            let dark = crate::ui::theme::is_dark(window);
            let background = palette.surface;
            let view = cx.entity();
            let rows = self
                .entries
                .iter()
                .enumerate()
                .map(|(index, entry)| match entry {
                    Entry::Separator => div()
                        .h(px(SEPARATOR_HEIGHT))
                        .flex()
                        .items_center()
                        .px(px(PADDING + 5.0))
                        .child(div().h(px(1.0)).w_full().bg(palette.separator))
                        .into_any_element(),
                    Entry::Item { label, .. } => {
                        let on = self.highlighted == Some(index);
                        div()
                            .id(index)
                            .h(px(ITEM_HEIGHT))
                            .flex()
                            .items_center()
                            .pl(px(TEXT_INSET))
                            .pr(px(TEXT_END))
                            .rounded(px(HIGHLIGHT_RADIUS))
                            .text_size(px(text::BODY))
                            .text_color(if on { palette.on_accent } else { palette.label })
                            .when(on, |this| this.bg(palette.blue))
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.highlighted != Some(index) {
                                    this.highlighted = Some(index);
                                    cx.notify();
                                }
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.close(Some(index), window, cx)
                                }),
                            )
                            .child(label.clone())
                            .into_any_element()
                    }
                });
            let shadows = vec![
                BoxShadow {
                    color: gpui_kit::hsla(0.0, 0.0, 0.0, if dark { 0.5 } else { 0.22 }),
                    offset: point(px(0.0), px(5.0)),
                    blur_radius: px(9.0),
                    spread_radius: px(-1.0),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::hsla(0.0, 0.0, 0.0, if dark { 0.6 } else { 0.18 }),
                    offset: point(px(0.0), px(0.0)),
                    blur_radius: px(1.0),
                    spread_radius: px(0.0),
                    inset: false,
                },
            ];
            div()
                .id("menu-window")
                .size_full()
                .p(px(SHADOW))
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::key))
                .child(
                    div()
                        .id("menu")
                        .size_full()
                        .rounded(px(RADIUS))
                        .bg(background)
                        .border_1()
                        .border_color(palette.stroke)
                        .shadow(shadows)
                        .p(px(PADDING - 1.0))
                        .font_family(crate::ui::theme::ui_font(cx))
                        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                            if !hovered && this.highlighted.is_some() {
                                this.highlighted = None;
                                cx.notify();
                            }
                        }))
                        .children(rows),
                )
                // A press outside the menu (delivered here by the grab)
                // closes it without choosing anything.
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds: Bounds<Pixels>, _, window, _| {
                            let menu = Bounds {
                                origin: bounds.origin + point(px(SHADOW), px(SHADOW)),
                                size: bounds.size - size(px(SHADOW * 2.0), px(SHADOW * 2.0)),
                            };
                            let view = view.clone();
                            window.on_mouse_event(
                                move |event: &MouseDownEvent, phase, window, cx| {
                                    if phase.bubble() && !menu.contains(&event.position) {
                                        view.update(cx, |this, cx| this.close(None, window, cx));
                                    }
                                },
                            );
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
        }
    }
}
