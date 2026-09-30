//! A slider drawn like AppKit's `NSSlider` at its small control size: a thin
//! capsule track, filled in the accent color up to a round white knob that
//! has the switch thumb's soft shadow. Clicking the track jumps the knob
//! there; dragging the knob or the track moves it. The behavior, including
//! the accessibility role and value, comes from GPUI Kit's base slider.

use crate::ui::theme::Palette;
use gpui_kit::{
    App, BoxShadow, Entity, Hsla, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window,
    base::{Slider, SliderIndicator, SliderThumb, SliderTrack, slider::SliderState},
    div, point,
    prelude::FluentBuilder as _,
    px, relative,
};

/// The track's thickness, in points.
const TRACK: f32 = 4.0;
/// The knob's diameter; it also sets the control's height.
const KNOB: f32 = 16.0;

#[derive(IntoElement)]
pub struct MacSlider {
    state: Entity<SliderState>,
    color: Hsla,
    disabled: bool,
    palette: Palette,
}

/// A slider over `state`, which holds a value from 0 to 1, filled in the
/// accent color.
pub fn mac_slider(state: &Entity<SliderState>, palette: Palette) -> MacSlider {
    MacSlider {
        state: state.clone(),
        color: palette.blue,
        disabled: false,
        palette,
    }
}

impl MacSlider {
    /// The filled part of the track.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = color;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for MacSlider {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = self.palette;
        let fraction = self.state.read(cx).percentage().end.clamp(0.0, 1.0);
        let shadow = |alpha: f32, y: f32, blur: f32| BoxShadow {
            color: gpui_kit::black().opacity(alpha),
            offset: point(px(0.0), px(y)),
            blur_radius: px(blur),
            spread_radius: px(0.0),
            inset: false,
        };

        let knob = SliderThumb::new(&self.state)
            .disabled(self.disabled)
            .absolute()
            .top(px((TRACK - KNOB) / 2.0))
            .left(relative(fraction))
            .ml(px(-KNOB / 2.0))
            .size(px(KNOB))
            .rounded_full()
            .bg(gpui_kit::white())
            .shadow(vec![shadow(0.16, 0.5, 2.0), shadow(0.05, 0.5, 2.5)]);
        // The indicator's bounds map the pointer to a value, so it spans the
        // knob's travel: the knob's center reaches its ends and the knob
        // itself stays inside the control.
        let bar = SliderIndicator::new(&self.state)
            .relative()
            .w_full()
            .h(px(TRACK))
            .rounded_full()
            .bg(palette.track)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .w(relative(fraction))
                    .rounded_full()
                    .bg(self.color),
            )
            .child(knob);

        Slider::new(&self.state)
            .disabled(self.disabled)
            .w_full()
            .h(px(KNOB))
            .flex()
            .items_center()
            .when(self.disabled, |slider| slider.opacity(0.5))
            .child(
                SliderTrack::new(&self.state)
                    .disabled(self.disabled)
                    .size_full()
                    .px(px(KNOB / 2.0))
                    .flex()
                    .items_center()
                    .child(bar),
            )
    }
}
