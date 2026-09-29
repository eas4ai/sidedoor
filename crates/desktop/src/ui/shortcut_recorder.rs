//! "Assign Shortcut": press the keys, see them as keycaps, save.

use crate::ui::theme::{Palette, text};
use domain::shortcut::Shortcut;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, Keystroke, Modifiers, ModifiersChangedEvent, ObjectFit, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    Subscription, TestSupportExt as _, Window, WindowControlArea, div, img,
    prelude::FluentBuilder as _, px, svg,
};
use std::{path::PathBuf, rc::Rc};

pub const CONTEXT: &str = "ShortcutRecorder";
/// Room above the content for the traffic lights, where they are inset.
const TOP: f32 = 18.0 + crate::ui::chrome::title_strip(22.0);
pub const WINDOW_SIZE: (f32, f32) = (400.0, 268.0 + crate::ui::chrome::title_strip(22.0));

/// What the recorder decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecorderEvent {
    Save(Shortcut),
    Remove,
    Cancel,
    /// The window's close button was used; it is already closing.
    Closed,
}

/// Checks a combination for conflicts; `Err` explains who has it.
pub type ConflictCheck = Rc<dyn Fn(&Shortcut, &mut App) -> Result<(), String>>;

pub struct ShortcutRecorder {
    title: SharedString,
    icon: Option<PathBuf>,
    glyph: SharedString,
    current: Option<Shortcut>,
    /// The last combination pressed, valid or not.
    pressed: Option<Shortcut>,
    /// Why `pressed` can't be saved.
    problem: Option<SharedString>,
    /// Modifiers held right now, shown before a key completes the shortcut.
    held: Modifiers,
    check: ConflictCheck,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RecorderEvent> for ShortcutRecorder {}

impl ShortcutRecorder {
    pub fn new(
        title: impl Into<SharedString>,
        icon: Option<PathBuf>,
        glyph: SharedString,
        current: Option<Shortcut>,
        check: ConflictCheck,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let view = cx.entity().downgrade();
        let closing = view.clone();
        window.on_window_should_close(cx, move |_, cx| {
            closing
                .update(cx, |_, cx| cx.emit(RecorderEvent::Closed))
                .ok();
            true
        });
        crate::ui::theme::sync_kit_theme(window, cx);
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, window, cx| {
                crate::ui::theme::sync_kit_theme(window, cx);
                cx.notify();
            }),
            // Every key belongs to the recording, even ones apps bind.
            cx.intercept_keystrokes(move |event, _, cx| {
                if !event
                    .context_stack
                    .iter()
                    .any(|context| context.contains(CONTEXT))
                {
                    return;
                }
                view.update(cx, |this, cx| this.press(&event.keystroke, cx))
                    .ok();
                cx.stop_propagation();
            }),
        ];
        Self {
            title: title.into(),
            icon,
            glyph,
            current,
            pressed: None,
            problem: None,
            held: Modifiers::default(),
            check,
            focus,
            _subscriptions: subscriptions,
        }
    }

    /// The combination Save would store.
    pub fn ready(&self) -> Option<&Shortcut> {
        self.pressed.as_ref().filter(|_| self.problem.is_none())
    }

    fn press(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let modifiers = keystroke.modifiers;
        let bare = !(modifiers.platform || modifiers.alt || modifiers.control || modifiers.shift);
        match keystroke.key.as_str() {
            "escape" if bare => return cx.emit(RecorderEvent::Cancel),
            "enter" if bare => {
                if let Some(shortcut) = self.ready().cloned() {
                    cx.emit(RecorderEvent::Save(shortcut));
                }
                return;
            }
            "backspace" | "delete" if bare => {
                self.pressed = None;
                self.problem = None;
                cx.notify();
                return;
            }
            _ => {}
        }
        let shortcut = Shortcut {
            control: modifiers.control,
            option: modifiers.alt,
            shift: modifiers.shift,
            command: modifiers.platform,
            key: keystroke.key.to_ascii_lowercase(),
        };
        self.problem = match shortcut.validate() {
            Err(problem) => Some(problem.to_string().into()),
            Ok(()) if Some(&shortcut) == self.current.as_ref() => None,
            Ok(()) => (self.check)(&shortcut, cx).err().map(Into::into),
        };
        self.pressed = Some(shortcut);
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if let Some(shortcut) = self.ready().cloned() {
            cx.emit(RecorderEvent::Save(shortcut));
        }
    }
}

fn keycap(label: String, palette: Palette, dim: bool) -> impl IntoElement {
    div()
        .min_w(px(44.0))
        .h(px(44.0))
        .px(px(10.0))
        .rounded(px(9.0))
        .bg(palette.keycap)
        .border_1()
        .border_color(palette.stroke)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(18.0))
        .font_weight(FontWeight::MEDIUM)
        .when(dim, |cap| cap.opacity(0.5))
        .child(label)
}

fn button(
    id: &'static str,
    label: &'static str,
    fill: Hsla,
    color: Hsla,
    enabled: bool,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .px(px(14.0))
        .py(px(5.0))
        .rounded(px(7.0))
        .bg(fill)
        .text_color(color)
        .text_size(px(text::BODY))
        .font_weight(FontWeight::MEDIUM)
        .when_else(
            enabled,
            |button| {
                button
                    .hover(|style| style.opacity(0.85))
                    .active(|style| style.opacity(0.7))
                    .on_click(move |_, _, cx: &mut App| on_click(cx))
            },
            |button| button.opacity(0.4),
        )
        .child(label)
}

impl Render for ShortcutRecorder {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::new(window, Default::default());
        let view = cx.entity();

        // What the key field shows: the pressed combination, the modifiers
        // being held, or the current shortcut, dimmed.
        let held = Shortcut {
            control: self.held.control,
            option: self.held.alt,
            shift: self.held.shift,
            command: self.held.platform,
            key: String::new(),
        };
        let holding = self.held.control || self.held.alt || self.held.shift || self.held.platform;
        let (caps, dim): (Vec<String>, bool) = match (&self.pressed, &self.current) {
            _ if holding && self.pressed.is_none() => {
                let mut caps = held.symbols();
                caps.pop();
                (caps, false)
            }
            (Some(pressed), _) => (pressed.symbols(), false),
            (None, Some(current)) => (current.symbols(), true),
            (None, None) => (Vec::new(), false),
        };

        let artwork = match &self.icon {
            Some(path) => img(path.clone())
                .size(px(40.0))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div()
                .size(px(36.0))
                .rounded(px(9.0))
                .bg(palette.fill)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path(self.glyph.clone())
                        .size(px(18.0))
                        .text_color(palette.label),
                )
                .into_any_element(),
        };

        let field = div()
            .id("key-field")
            .test_support()
            .h(px(92.0))
            .rounded(px(12.0))
            .bg(palette.fill)
            .border_1()
            .border_color(if self.problem.is_some() {
                palette.orange
            } else {
                palette.separator
            })
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .children(caps.into_iter().map(|cap| keycap(cap, palette, dim)))
            .when(
                self.pressed.is_none() && !holding && self.current.is_none(),
                |field| {
                    field.child(
                        div()
                            .text_color(palette.tertiary)
                            .child("Press the keys you want to use"),
                    )
                },
            );

        let status = match (&self.problem, self.ready()) {
            (Some(problem), _) => div().text_color(palette.orange).child(problem.clone()),
            (None, Some(_)) => div()
                .text_color(palette.secondary)
                .child("Press Return to save it."),
            (None, None) => div().text_color(palette.secondary).child(if cfg!(windows) {
                "Use Ctrl, Alt or Win with any key, or an F-key on its own."
            } else if cfg!(target_os = "linux") {
                "Use Ctrl, Alt or Super with any key, or an F-key on its own."
            } else {
                "Use ⌘, ⌥ or ⌃ with any key, or an F-key on its own."
            }),
        };

        let (save, cancel, remove) = (view.clone(), view.clone(), view);
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.held = event.modifiers;
                cx.notify();
            }))
            .size_full()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .px(px(20.0))
            .pt(px(TOP))
            .pb(px(18.0))
            .text_color(palette.label)
            .text_size(px(text::BODY))
            .child(
                div()
                    .window_control_area(WindowControlArea::Drag)
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(artwork)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(text::TITLE3))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.title.clone()),
                            )
                            .child(
                                div()
                                    .text_color(palette.secondary)
                                    .child("Choose keys that open it from any app."),
                            ),
                    ),
            )
            .child(field)
            .child(status.text_size(px(text::CALLOUT)))
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .children(self.current.is_some().then(|| {
                        button(
                            "remove-shortcut",
                            "Remove",
                            palette.fill,
                            palette.red,
                            true,
                            move |cx| remove.update(cx, |_, cx| cx.emit(RecorderEvent::Remove)),
                        )
                    }))
                    .child(div().flex_1())
                    .child(button(
                        "cancel-shortcut",
                        "Cancel",
                        palette.fill,
                        palette.label,
                        true,
                        move |cx| cancel.update(cx, |_, cx| cx.emit(RecorderEvent::Cancel)),
                    ))
                    .child(button(
                        "save-shortcut",
                        "Save",
                        palette.blue,
                        palette.on_accent,
                        self.ready().is_some(),
                        move |cx| save.update(cx, |this, cx| this.save(cx)),
                    )),
            )
    }
}
