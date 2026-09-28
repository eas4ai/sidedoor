//! Dock layout and auto-hide rules, in AppKit screen coordinates
//! (origin at the bottom-left of the main display, y grows upward).

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Cross-axis size of the dock.
pub const DOCK_THICKNESS: f64 = 60.0;
/// Main-axis size of one slot (app icon or widget).
pub const SLOT: f64 = 52.0;
/// Space before the first and after the last slot.
pub const DOCK_PADDING: f64 = 8.0;
pub const DOCK_RADIUS: f64 = 20.0;
pub const CARD_RADIUS: f64 = 16.0;
/// Distance between the dock and the screen edge.
const EDGE_INSET: f64 = 6.0;
/// Distance between the dock and a card's arrow tip.
const CARD_GAP: f64 = 3.0;
/// How far the pointer may stray from the dock before it counts as outside.
const HOVER_SLOP: f64 = 16.0;
/// Widest card, used for the area that keeps the dock open while a card shows.
pub const CARD_ZONE: f64 = 340.0;
/// How long the pointer must stay outside before the dock hides.
pub const HIDE_DELAY: Duration = Duration::from_millis(450);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    #[default]
    Right,
    Bottom,
}

impl Edge {
    /// Whether slots stack vertically.
    pub fn is_vertical(self) -> bool {
        !matches!(self, Self::Bottom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn mid_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn mid_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.x <= self.max_x() && point.y >= self.y && point.y <= self.max_y()
    }

    pub fn inflate(&self, by: f64) -> Self {
        Self::new(
            self.x - by,
            self.y - by,
            self.width + by * 2.0,
            self.height + by * 2.0,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// The display the dock lives on: its full frame and the part not covered by
/// the menu bar or the system Dock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub frame: Rect,
    pub visible: Rect,
}

/// Size of the dock holding `slots` items.
pub fn dock_size(edge: Edge, slots: usize) -> (f64, f64) {
    let length = DOCK_PADDING * 2.0 + SLOT * slots.max(1) as f64;
    if edge.is_vertical() {
        (DOCK_THICKNESS, length)
    } else {
        (length, DOCK_THICKNESS)
    }
}

/// Frame of the dock while it is shown: floating just off its edge and
/// centered along it.
pub fn dock_frame(screen: Screen, edge: Edge, slots: usize) -> Rect {
    let (width, height) = dock_size(edge, slots);
    let visible = screen.visible;
    match edge {
        Edge::Right => {
            let height = height.min(visible.height);
            let x = screen.frame.max_x() - EDGE_INSET - width;
            Rect::new(x, visible.mid_y() - height / 2.0, width, height)
        }
        Edge::Left => {
            let height = height.min(visible.height);
            let x = screen.frame.x + EDGE_INSET;
            Rect::new(x, visible.mid_y() - height / 2.0, width, height)
        }
        Edge::Bottom => {
            let width = width.min(visible.width);
            let y = visible.y + EDGE_INSET;
            Rect::new(visible.mid_x() - width / 2.0, y, width, height)
        }
    }
}

/// Frame of the dock while it is hidden: slid past its edge.
pub fn hidden_dock_frame(screen: Screen, edge: Edge, slots: usize) -> Rect {
    let shown = dock_frame(screen, edge, slots);
    match edge {
        Edge::Right => Rect {
            x: screen.frame.max_x() + 1.0,
            ..shown
        },
        Edge::Left => Rect {
            x: screen.frame.x - shown.width - 1.0,
            ..shown
        },
        Edge::Bottom => Rect {
            y: screen.frame.y - shown.height - 1.0,
            ..shown
        },
    }
}

/// Center of slot `index` in screen coordinates.
pub fn slot_center(dock: Rect, edge: Edge, index: usize) -> Point {
    let offset = DOCK_PADDING + SLOT * index as f64 + SLOT / 2.0;
    if edge.is_vertical() {
        Point {
            x: dock.mid_x(),
            y: dock.max_y() - offset,
        }
    } else {
        Point {
            x: dock.x + offset,
            y: dock.mid_y(),
        }
    }
}

/// Which side of a card window carries the arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowSide {
    Left,
    Right,
    Bottom,
}

impl Edge {
    /// Cards open away from the edge, so the arrow sits on the dock's side.
    pub fn arrow_side(self) -> ArrowSide {
        match self {
            Self::Right => ArrowSide::Right,
            Self::Left => ArrowSide::Left,
            Self::Bottom => ArrowSide::Bottom,
        }
    }
}

/// The shape of a card: its corner radius and the arrow toward its item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardShape {
    pub radius: f64,
    /// Half the arrow's width where it meets the card.
    pub arrow_half_width: f64,
    /// How far the arrow reaches out from the card.
    pub arrow_depth: f64,
}

/// Cards with content: generous corners, a clear arrow.
pub const CARD_SHAPE: CardShape = CardShape {
    radius: CARD_RADIUS,
    arrow_half_width: 9.0,
    arrow_depth: 8.0,
};

/// One-line tooltips: tighter corners leave a straight edge for the arrow.
pub const TOOLTIP_SHAPE: CardShape = CardShape {
    radius: 8.0,
    arrow_half_width: 6.0,
    arrow_depth: 6.0,
};

/// Cards this short are tooltips.
const TOOLTIP_MAX_HEIGHT: f64 = 40.0;

impl CardShape {
    fn for_height(height: f64) -> Self {
        if height <= TOOLTIP_MAX_HEIGHT {
            TOOLTIP_SHAPE
        } else {
            CARD_SHAPE
        }
    }
}

/// Where a card window goes and where its arrow points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardPlacement {
    /// The window frame, arrow included.
    pub frame: Rect,
    pub side: ArrowSide,
    pub shape: CardShape,
    /// Arrow center along its side, from the window's top (vertical sides) or
    /// left (bottom side), in window points.
    pub arrow_offset: f64,
}

/// One step of an outline, in window points from the top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathStep {
    Move(Point),
    Line(Point),
    Cubic {
        control_a: Point,
        control_b: Point,
        to: Point,
    },
    Close,
}

/// Control-point distance that makes a cubic Bézier follow a quarter circle.
const QUARTER_CIRCLE: f64 = 0.552_284_75;

fn at(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn lerp(a: Point, b: Point, t: f64) -> Point {
    at(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

impl CardPlacement {
    /// The card body inside the window, in window points from the top-left.
    pub fn body(&self) -> Rect {
        let (width, height) = (self.frame.width, self.frame.height);
        let depth = self.shape.arrow_depth;
        match self.side {
            ArrowSide::Right => Rect::new(0.0, 0.0, width - depth, height),
            ArrowSide::Left => Rect::new(depth, 0.0, width - depth, height),
            ArrowSide::Bottom => Rect::new(0.0, 0.0, width, height - depth),
        }
    }

    /// Corner radius that fits the body.
    pub fn radius(&self) -> f64 {
        let body = self.body();
        self.shape
            .radius
            .min(body.width / 2.0)
            .min(body.height / 2.0)
    }

    /// The card's silhouette, body and arrow as one closed shape, traced
    /// clockwise from the top-left corner. The glass is masked to it and the
    /// hairline is stroked along it, so the arrow is part of the card.
    pub fn outline(&self) -> Vec<PathStep> {
        let body = self.body();
        let (x0, y0, x1, y1) = (body.x, body.y, body.max_x(), body.max_y());
        let r = self.radius();
        let k = r * QUARTER_CIRCLE;
        let offset = self.arrow_offset;
        let mut steps = vec![PathStep::Move(at(x0 + r, y0))];

        // Top edge, then the top-right corner.
        steps.push(PathStep::Line(at(x1 - r, y0)));
        steps.push(PathStep::Cubic {
            control_a: at(x1 - r + k, y0),
            control_b: at(x1, y0 + r - k),
            to: at(x1, y0 + r),
        });
        // Right edge, heading down.
        if self.side == ArrowSide::Right {
            self.arrow(
                &mut steps,
                at(x1, y0 + r),
                (0.0, 1.0),
                (1.0, 0.0),
                offset - (y0 + r),
            );
        }
        steps.push(PathStep::Line(at(x1, y1 - r)));
        steps.push(PathStep::Cubic {
            control_a: at(x1, y1 - r + k),
            control_b: at(x1 - r + k, y1),
            to: at(x1 - r, y1),
        });
        // Bottom edge, heading left.
        if self.side == ArrowSide::Bottom {
            self.arrow(
                &mut steps,
                at(x1 - r, y1),
                (-1.0, 0.0),
                (0.0, 1.0),
                (x1 - r) - offset,
            );
        }
        steps.push(PathStep::Line(at(x0 + r, y1)));
        steps.push(PathStep::Cubic {
            control_a: at(x0 + r - k, y1),
            control_b: at(x0, y1 - r + k),
            to: at(x0, y1 - r),
        });
        // Left edge, heading up.
        if self.side == ArrowSide::Left {
            self.arrow(
                &mut steps,
                at(x0, y1 - r),
                (0.0, -1.0),
                (-1.0, 0.0),
                (y1 - r) - offset,
            );
        }
        steps.push(PathStep::Line(at(x0, y0 + r)));
        steps.push(PathStep::Cubic {
            control_a: at(x0, y0 + r - k),
            control_b: at(x0 + r - k, y0),
            to: at(x0 + r, y0),
        });
        steps.push(PathStep::Close);
        steps
    }

    /// Adds the arrow to a straight edge that starts at `start` and runs
    /// along `direction`; `outward` points away from the card. `along` is the
    /// arrow's center measured from `start`.
    fn arrow(
        &self,
        steps: &mut Vec<PathStep>,
        start: Point,
        direction: (f64, f64),
        outward: (f64, f64),
        along: f64,
    ) {
        let half = self.shape.arrow_half_width;
        let depth = self.shape.arrow_depth;
        let point = |t: f64, out: f64| {
            at(
                start.x + direction.0 * t + outward.0 * out,
                start.y + direction.1 * t + outward.1 * out,
            )
        };
        // Soft joins where the arrow leaves and rejoins the edge, and a
        // rounded tip, like a macOS popover.
        let fillet = half * 0.35;
        let base_a = point(along - half, 0.0);
        let base_b = point(along + half, 0.0);
        let tip = point(along, depth);
        steps.push(PathStep::Line(point(along - half - fillet, 0.0)));
        steps.push(PathStep::Cubic {
            control_a: base_a,
            control_b: base_a,
            to: lerp(base_a, tip, 0.25),
        });
        steps.push(PathStep::Line(lerp(base_a, tip, 0.78)));
        steps.push(PathStep::Cubic {
            control_a: tip,
            control_b: tip,
            to: lerp(base_b, tip, 0.78),
        });
        steps.push(PathStep::Line(lerp(base_b, tip, 0.25)));
        steps.push(PathStep::Cubic {
            control_a: base_b,
            control_b: base_b,
            to: point(along + half + fillet, 0.0),
        });
    }
}

impl ArrowSide {
    /// A `distance`-point step toward the dock, in screen coordinates.
    pub fn toward_dock(self, distance: f64) -> (f64, f64) {
        match self {
            Self::Right => (distance, 0.0),
            Self::Left => (-distance, 0.0),
            Self::Bottom => (0.0, -distance),
        }
    }
}

/// Where a card of `next`'s size starts when it glides over from `previous`:
/// its dock-facing side and center line stay where the old card's were.
pub fn glide_start(previous: Rect, next: Rect, side: ArrowSide) -> Rect {
    let (width, height) = (next.width, next.height);
    match side {
        ArrowSide::Right => Rect::new(
            previous.max_x() - width,
            previous.mid_y() - height / 2.0,
            width,
            height,
        ),
        ArrowSide::Left => Rect::new(previous.x, previous.mid_y() - height / 2.0, width, height),
        ArrowSide::Bottom => Rect::new(previous.mid_x() - width / 2.0, previous.y, width, height),
    }
}

/// Places a `width`×`height` card (arrow excluded) for slot `index`: beside
/// the dock, centered on the slot, and kept inside the visible area.
pub fn card_placement(
    screen: Screen,
    dock: Rect,
    edge: Edge,
    index: usize,
    width: f64,
    height: f64,
) -> CardPlacement {
    let slot = slot_center(dock, edge, index);
    let visible = screen.visible.inflate(-EDGE_INSET);
    let shape = CardShape::for_height(height);
    let clamp = |value: f64, min: f64, max: f64| value.clamp(min, max.max(min));
    // Keep the arrow on the straight part of its edge; a card too short for
    // that gets its arrow centered.
    let arrow_on = |along: f64, length: f64| {
        let radius = shape.radius.min(width / 2.0).min(height / 2.0);
        let margin = radius + shape.arrow_half_width * 1.35;
        if length < margin * 2.0 {
            length / 2.0
        } else {
            along.clamp(margin, length - margin)
        }
    };

    if edge.is_vertical() {
        let window_width = width + shape.arrow_depth;
        let x = match edge {
            Edge::Right => dock.x - CARD_GAP - window_width,
            _ => dock.max_x() + CARD_GAP,
        };
        let y = clamp(slot.y - height / 2.0, visible.y, visible.max_y() - height);
        let frame = Rect::new(x, y, window_width, height);
        CardPlacement {
            frame,
            side: edge.arrow_side(),
            shape,
            arrow_offset: arrow_on(frame.max_y() - slot.y, height),
        }
    } else {
        let window_height = height + shape.arrow_depth;
        let x = clamp(slot.x - width / 2.0, visible.x, visible.max_x() - width);
        let frame = Rect::new(x, dock.max_y() + CARD_GAP, width, window_height);
        CardPlacement {
            frame,
            side: ArrowSide::Bottom,
            shape,
            arrow_offset: arrow_on(slot.x - x, width),
        }
    }
}

/// The area beside the dock where an open card may be; the pointer in it
/// keeps the dock shown.
pub fn card_zone(screen: Screen, dock: Rect, edge: Edge) -> Rect {
    let visible = screen.visible;
    match edge {
        Edge::Right => Rect::new(dock.x - CARD_ZONE, visible.y, CARD_ZONE, visible.height),
        Edge::Left => Rect::new(dock.max_x(), visible.y, CARD_ZONE, visible.height),
        Edge::Bottom => Rect::new(visible.x, dock.max_y(), visible.width, CARD_ZONE),
    }
}

/// Tracks whether the dock is shown, from polled pointer positions.
///
/// The dock appears when the pointer touches the screen edge beside it, and
/// hides once the pointer has stayed away from it for [`HIDE_DELAY`].
#[derive(Debug, Default)]
pub struct Reveal {
    shown: bool,
    outside_since: Option<Instant>,
}

impl Reveal {
    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// Shows the dock now and keeps it up for `duration` unless the pointer
    /// arrives, as when a shortcut peeks at a widget.
    pub fn show_for(&mut self, now: Instant, duration: Duration) {
        self.shown = true;
        self.outside_since = Some(now + duration.saturating_sub(HIDE_DELAY));
    }

    /// Feeds one pointer sample. `keep` is an extra area, such as an open
    /// card, that also counts as inside. Returns `true` when visibility changed.
    pub fn update(
        &mut self,
        pointer: Point,
        screen: Screen,
        edge: Edge,
        dock: Rect,
        keep: Option<Rect>,
        now: Instant,
    ) -> bool {
        let frame = screen.frame;
        if !self.shown {
            let along_y =
                pointer.y >= dock.y - HOVER_SLOP && pointer.y <= dock.max_y() + HOVER_SLOP;
            let along_x =
                pointer.x >= dock.x - HOVER_SLOP && pointer.x <= dock.max_x() + HOVER_SLOP;
            let (at_edge, in_band) = match edge {
                Edge::Right => (
                    pointer.x >= frame.max_x() - 2.0 && pointer.x <= frame.max_x(),
                    along_y,
                ),
                Edge::Left => (pointer.x <= frame.x + 2.0 && pointer.x >= frame.x, along_y),
                Edge::Bottom => (pointer.y <= frame.y + 2.0 && pointer.y >= frame.y, along_x),
            };
            if at_edge && in_band {
                self.shown = true;
                self.outside_since = None;
                return true;
            }
            return false;
        }

        // The dock, grown to reach the screen edge behind it.
        let edge_strip = match edge {
            Edge::Right => Rect::new(dock.x, dock.y, frame.max_x() - dock.x, dock.height),
            Edge::Left => Rect::new(frame.x, dock.y, dock.max_x() - frame.x, dock.height),
            Edge::Bottom => Rect::new(dock.x, frame.y, dock.width, dock.max_y() - frame.y),
        };
        let inside = edge_strip.inflate(HOVER_SLOP).contains(pointer)
            || keep.is_some_and(|area| area.contains(pointer));
        if inside {
            self.outside_since = None;
            return false;
        }
        let since = *self.outside_since.get_or_insert(now);
        if now.saturating_duration_since(since) >= HIDE_DELAY {
            self.shown = false;
            self.outside_since = None;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Screen {
        Screen {
            frame: Rect::new(0.0, 0.0, 1512.0, 982.0),
            visible: Rect::new(0.0, 0.0, 1512.0, 949.0),
        }
    }

    #[test]
    fn right_dock_is_centered_against_the_edge() {
        let dock = dock_frame(screen(), Edge::Right, 4);
        assert_eq!(dock.height, dock_size(Edge::Right, 4).1);
        assert_eq!(dock.max_x(), 1512.0 - EDGE_INSET);
        assert!((dock.mid_y() - 949.0 / 2.0).abs() < 1e-9);
        assert!(hidden_dock_frame(screen(), Edge::Right, 4).x > 1512.0);
    }

    #[test]
    fn left_and_bottom_docks_hide_past_their_edges() {
        let left = dock_frame(screen(), Edge::Left, 3);
        assert_eq!(left.x, EDGE_INSET);
        assert!(hidden_dock_frame(screen(), Edge::Left, 3).max_x() < 0.0);

        let bottom = dock_frame(screen(), Edge::Bottom, 3);
        assert_eq!(bottom.height, DOCK_THICKNESS);
        assert!((bottom.mid_x() - 756.0).abs() < 1e-9);
        assert!(hidden_dock_frame(screen(), Edge::Bottom, 3).max_y() < 0.0);
    }

    #[test]
    fn slots_run_top_to_bottom_or_left_to_right() {
        let dock = dock_frame(screen(), Edge::Right, 3);
        let first = slot_center(dock, Edge::Right, 0);
        let second = slot_center(dock, Edge::Right, 1);
        assert_eq!(first.y - second.y, SLOT);
        assert_eq!(dock.max_y() - first.y, DOCK_PADDING + SLOT / 2.0);

        let dock = dock_frame(screen(), Edge::Bottom, 3);
        let first = slot_center(dock, Edge::Bottom, 0);
        let second = slot_center(dock, Edge::Bottom, 1);
        assert_eq!(second.x - first.x, SLOT);
    }

    #[test]
    fn cards_open_away_from_the_edge_with_the_arrow_on_the_slot() {
        let dock = dock_frame(screen(), Edge::Right, 5);
        let card = card_placement(screen(), dock, Edge::Right, 2, 280.0, 120.0);
        assert!(card.frame.max_x() < dock.x);
        assert_eq!(card.side, ArrowSide::Right);
        let slot = slot_center(dock, Edge::Right, 2);
        assert!((card.frame.max_y() - card.arrow_offset - slot.y).abs() < 1e-9);
        assert_eq!(card.body().width, 280.0);

        let dock = dock_frame(screen(), Edge::Left, 5);
        let card = card_placement(screen(), dock, Edge::Left, 0, 280.0, 120.0);
        assert!(card.frame.x > dock.max_x());
        assert_eq!(card.body().x, CARD_SHAPE.arrow_depth);

        let dock = dock_frame(screen(), Edge::Bottom, 5);
        let card = card_placement(screen(), dock, Edge::Bottom, 4, 280.0, 120.0);
        assert!(card.frame.y > dock.max_y());
        assert_eq!(card.body().height, 120.0);
    }

    #[test]
    fn cards_stay_on_screen_and_arrows_stay_on_the_card() {
        let dock = dock_frame(screen(), Edge::Right, 12);
        let card = card_placement(screen(), dock, Edge::Right, 11, 280.0, 400.0);
        assert!(card.frame.y >= screen().visible.y);
        assert!(card.frame.max_y() <= screen().visible.max_y());
        assert!(card.arrow_offset <= 400.0 - CARD_RADIUS - CARD_SHAPE.arrow_half_width);
    }

    #[test]
    fn tooltips_get_a_centered_arrow_on_a_straight_edge() {
        let dock = dock_frame(screen(), Edge::Right, 5);
        let tip = card_placement(screen(), dock, Edge::Right, 1, 70.0, 28.0);
        assert_eq!(tip.shape, TOOLTIP_SHAPE);
        assert_eq!(tip.arrow_offset, 14.0);
        // The arrow's base fits between the rounded corners.
        let straight = 28.0 - 2.0 * tip.radius();
        assert!(2.0 * TOOLTIP_SHAPE.arrow_half_width <= straight);
    }

    #[test]
    fn outlines_close_inside_the_window_and_reach_its_arrow_edge() {
        let dock = dock_frame(screen(), Edge::Right, 5);
        for (edge, height) in [
            (Edge::Right, 28.0),
            (Edge::Left, 190.0),
            (Edge::Bottom, 120.0),
        ] {
            let dock = if edge == Edge::Right {
                dock
            } else {
                dock_frame(screen(), edge, 5)
            };
            let card = card_placement(screen(), dock, edge, 2, 280.0, height);
            let outline = card.outline();
            assert!(matches!(outline.first(), Some(PathStep::Move(_))));
            assert_eq!(outline.last(), Some(&PathStep::Close));
            let points: Vec<Point> = outline
                .iter()
                .filter_map(|step| match step {
                    PathStep::Move(p) | PathStep::Line(p) => Some(*p),
                    PathStep::Cubic { to, .. } => Some(*to),
                    PathStep::Close => None,
                })
                .collect();
            let (w, h) = (card.frame.width, card.frame.height);
            assert!(
                points
                    .iter()
                    .all(|p| p.x >= -1e-9 && p.x <= w + 1e-9 && p.y >= -1e-9 && p.y <= h + 1e-9)
            );
            let depth = card.shape.arrow_depth;
            let reach = match card.side {
                ArrowSide::Right => points.iter().map(|p| p.x).fold(0.0, f64::max) - (w - depth),
                ArrowSide::Left => depth - points.iter().map(|p| p.x).fold(w, f64::min),
                ArrowSide::Bottom => points.iter().map(|p| p.y).fold(0.0, f64::max) - (h - depth),
            };
            // The rounded tip stops just short of the full depth.
            assert!(
                reach > depth * 0.7 && reach <= depth + 1e-9,
                "{edge:?}: reach {reach}"
            );
        }
    }

    #[test]
    fn glides_keep_the_dock_side_and_center() {
        let previous = Rect::new(1300.0, 500.0, 80.0, 28.0);
        let next = Rect::new(1100.0, 420.0, 307.0, 190.0);
        let start = glide_start(previous, next, ArrowSide::Right);
        assert_eq!(start.max_x(), previous.max_x());
        assert_eq!(start.mid_y(), previous.mid_y());
        assert_eq!((start.width, start.height), (next.width, next.height));
        let start = glide_start(previous, next, ArrowSide::Bottom);
        assert_eq!((start.mid_x(), start.y), (previous.mid_x(), previous.y));
    }

    #[test]
    fn reveals_at_the_edge_and_hides_after_the_delay() {
        let (screen, edge) = (screen(), Edge::Right);
        let dock = dock_frame(screen, edge, 4);
        let start = Instant::now();
        let mut reveal = Reveal::default();

        let beside = Point {
            x: 1400.0,
            y: dock.mid_y(),
        };
        assert!(!reveal.update(beside, screen, edge, dock, None, start));

        let at_edge = Point {
            x: 1511.5,
            y: dock.mid_y(),
        };
        assert!(reveal.update(at_edge, screen, edge, dock, None, start));
        assert!(reveal.is_shown());

        let on_dock = Point {
            x: dock.x + 10.0,
            y: dock.mid_y(),
        };
        assert!(!reveal.update(on_dock, screen, edge, dock, None, start));

        let away = Point { x: 600.0, y: 300.0 };
        let left_at = start + Duration::from_secs(2);
        assert!(!reveal.update(away, screen, edge, dock, None, left_at));
        assert!(reveal.update(away, screen, edge, dock, None, left_at + HIDE_DELAY));
        assert!(!reveal.is_shown());
    }

    #[test]
    fn an_open_card_keeps_the_dock_shown() {
        let (screen, edge) = (screen(), Edge::Right);
        let dock = dock_frame(screen, edge, 4);
        let zone = card_zone(screen, dock, edge);
        let start = Instant::now();
        let mut reveal = Reveal::default();
        let at_edge = Point {
            x: 1511.5,
            y: dock.mid_y(),
        };
        reveal.update(at_edge, screen, edge, dock, None, start);

        let on_card = Point {
            x: dock.x - 150.0,
            y: dock.mid_y(),
        };
        let later = start + HIDE_DELAY * 3;
        assert!(!reveal.update(on_card, screen, edge, dock, Some(zone), start));
        assert!(!reveal.update(on_card, screen, edge, dock, Some(zone), later));
        assert!(reveal.is_shown());
    }

    #[test]
    fn a_peek_stays_up_for_its_duration() {
        let (screen, edge) = (screen(), Edge::Right);
        let dock = dock_frame(screen, edge, 3);
        let away = Point { x: 400.0, y: 400.0 };
        let start = Instant::now();
        let mut reveal = Reveal::default();
        reveal.show_for(start, Duration::from_secs(3));
        assert!(reveal.is_shown());
        assert!(!reveal.update(
            away,
            screen,
            edge,
            dock,
            None,
            start + Duration::from_secs(2)
        ));
        assert!(reveal.update(
            away,
            screen,
            edge,
            dock,
            None,
            start + Duration::from_secs(3)
        ));
    }

    #[test]
    fn bottom_edge_reveals_along_its_band_only() {
        let (screen, edge) = (screen(), Edge::Bottom);
        let dock = dock_frame(screen, edge, 3);
        let mut reveal = Reveal::default();
        let corner = Point { x: 5.0, y: 0.5 };
        assert!(!reveal.update(corner, screen, edge, dock, None, Instant::now()));
        let below = Point {
            x: dock.mid_x(),
            y: 0.5,
        };
        assert!(reveal.update(below, screen, edge, dock, None, Instant::now()));
    }
}
