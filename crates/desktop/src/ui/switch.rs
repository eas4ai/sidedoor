//! A switch drawn like AppKit's `NSSwitch` at its small control size, as
//! System Settings uses in its forms. The sizes and colors were measured from
//! the system control: a capsule track with a white capsule thumb, a soft
//! shadow under the thumb, and the accent color when on.

use crate::ui::theme::Palette;
use domain::motion::{self, SWITCH};
use gpui_kit::{
    App, BoxShadow, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    base::{Transition, transition},
    div, point,
    prelude::FluentBuilder as _,
    px,
};
use std::rc::Rc;

/// The track, in points.
const TRACK: (f32, f32) = (44.0, 20.0);
/// The thumb at rest, 2pt in from every edge of the track.
const THUMB: (f32, f32) = (26.0, 16.0);
const INSET: f32 = 2.0;
/// How much wider the thumb gets while pressed, stretching toward the middle.
const STRETCH: f32 = 4.0;
/// Groups the thumb with the control so pressing anywhere stretches it.
const GROUP: &str = "mac-switch";

type ChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct MacSwitch {
    id: ElementId,
    checked: bool,
    disabled: bool,
    palette: Palette,
    label: Option<SharedString>,
    on_change: Option<ChangeHandler>,
}

/// A switch, off and enabled until told otherwise.
pub fn mac_switch(id: impl Into<ElementId>, palette: Palette) -> MacSwitch {
    MacSwitch {
        id: id.into(),
        checked: false,
        disabled: false,
        palette,
        label: None,
        on_change: None,
    }
}

impl MacSwitch {
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// What VoiceOver reads when the row doesn't name the switch.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Called with the value the user switched to.
    pub fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for MacSwitch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = self.palette;
        let target = if self.checked { 1.0 } else { 0.0 };
        // How far on the switch is, 0–1; it can overshoot a little on the way.
        let on = if palette.reduce_motion {
            target
        } else {
            transition(
                ElementId::from((self.id.clone(), "thumb")),
                target,
                Transition::new(SWITCH.duration).ease(|t| motion::sample(SWITCH.curve, t)),
                window,
                cx,
            )
        };
        let travel = TRACK.0 - THUMB.0 - INSET * 2.0;
        let left = INSET + travel * on;
        let pressed_left = if self.checked { left - STRETCH } else { left };
        let shadow = |alpha: f32, y: f32, blur: f32| BoxShadow {
            color: gpui_kit::black().opacity(alpha),
            offset: point(px(0.0), px(y)),
            blur_radius: px(blur),
            spread_radius: px(0.0),
            inset: false,
        };

        let thumb = div()
            .id("thumb")
            .absolute()
            .top(px(INSET))
            .left(px(left))
            .w(px(THUMB.0))
            .h(px(THUMB.1))
            .rounded_full()
            .bg(gpui_kit::white())
            .shadow(vec![shadow(0.16, 0.5, 2.0), shadow(0.05, 0.5, 2.5)])
            .when(!self.disabled, |thumb| {
                thumb.group_active(GROUP, move |style| {
                    style.w(px(THUMB.0 + STRETCH)).left(px(pressed_left))
                })
            });
        let track = div()
            .relative()
            .size_full()
            .rounded_full()
            .bg(palette.switch_off)
            // The on color fades in over the off track as the thumb travels.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .bg(palette.blue)
                    .opacity(on.clamp(0.0, 1.0)),
            )
            .child(thumb);

        let on_change = self.on_change;
        gpui_kit::base::Switch::new(self.id)
            .checked(self.checked)
            .disabled(self.disabled)
            .when_some(self.label, |switch, label| {
                switch.accessibility_label(label)
            })
            .when_some(on_change, |switch, on_change| {
                switch.on_change(move |checked, _, window, cx| on_change(checked, window, cx))
            })
            .group(GROUP)
            .flex_shrink_0()
            .w(px(TRACK.0))
            .h(px(TRACK.1))
            .when(self.disabled, |switch| switch.opacity(0.5))
            .child(track)
    }
}
