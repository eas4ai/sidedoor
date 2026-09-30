# Sidedoor design

Sidedoor is a second dock that lives at the edge of the screen. It holds apps,
a few built-in widgets and plugins, and it hides until you reach for it.

This document explains what we're going for and why. The exact numbers live in
code. `crates/desktop/src/ui/theme.rs` has colors and type, `crates/domain/src/motion.rs` has timing, and
`crates/domain/src/geometry.rs` has sizes and shapes. If this file and the code disagree, the
code wins and this file needs an update.

## Philosophy

### It should look like Apple shipped it

Sidedoor has no brand color, no custom font and no signature look. It borrows
everything it can from macOS: the system materials, the AppKit semantic colors,
the Mac text sizes and the popover shape. Someone who opens it for the first
time should assume it came with the OS.

When we're unsure how something should look, we check how Finder, Control
Center, the Dock or System Settings does it, and do that. Inventing a new
pattern needs a reason the system pattern can't cover.

### It never gets in the way

The dock is a tool you use for two seconds while doing something else. So:

- It stays off screen until the pointer touches its edge.
- Clicking the dock or a card never takes focus away from the app you're in.
  Both are non-activating panels.
- When something does need the keyboard, like Clipboard History or Settings,
  it remembers which app you came from and hands focus back when it closes.
  After picking a clip you're back in your editor, ready to paste.
- It leaves faster than it arrives. Nobody wants to wait for a thing to go away.

### It reacts to intent, not to accidents

A pointer drifting past the edge shouldn't pop a dock in your face, and a
pointer that slips a few points off a card shouldn't slam it shut. The reveal
logic has deliberate slack built in:

- The dock appears only when the pointer is within 2 points of the screen edge
  and within the dock's band along it.
- Once shown, the dock stays while the pointer is within 16 points of it, or
  anywhere in the area where a card can open.
- It hides only after the pointer has been away for 450 ms.
- A card waits 160 ms before closing, so the pointer can travel from an icon
  onto its card.

### One motion system

The dock, its icons, its cards and its windows share one set of curves. Native
AppKit animations and GPUI animations sample the same Bézier curves, so a card
sliding in natively and its content fading up in GPUI feel like a single
movement. New animations reuse a curve from `motion.rs`. If none fits, add a
named one there instead of writing numbers inline.

### The user's system settings are the spec

Light and dark mode, Reduce Motion, Reduce Transparency and Increase Contrast
are part of the design. Each one changes how Sidedoor draws, and every screen
has to look right under all of them. See [Accessibility](#accessibility).

### Plugins are first-class and look native

Plugins are written in TSX, but they don't get a web view. The app draws their
tree with real GPUI elements and the same Rust components the built-in widgets
use. A plugin card should be indistinguishable from the Weather card. That's
why the SDK exposes palette tokens instead of encouraging hex colors, and why
its `transition` prop uses the app's own curves.

## How it should feel

Here is the whole interaction, as it should play out:

1. You push the pointer against the right edge. The dock slides out in 420 ms
   with a soft overshoot, like it was waiting just past the bezel.
2. Its icons follow it in, each one 24 ms after its neighbor, travelling 16
   points from the edge with a slightly springier curve than the dock. The
   dock arrives as one piece and the icons settle into it.
3. You move along the dock. The icon under the pointer grows up to 114%, and
   its neighbors grow a little less, like the macOS Dock with magnification on.
4. You rest on a widget. Its card grows out of the icon from 86% scale, the way
   an `NSPopover` does, with an arrow pointing back at the icon. Its content
   fades up 3 points as it lands.
5. You sweep to the next item. The card doesn't close and reopen. It glides to
   the new item in 170 ms on a plain ease-out, so a fast sweep along the dock
   doesn't wobble.
6. You press an icon. It dips to 86% while held, so the click feels physical.
7. You move away. After 450 ms the dock tucks back in 220 ms, accelerating
   out, with no bounce.

Arrivals get a little spring. Departures are quick and plain. Moves between
states are smooth and never overshoot.

## Materials

Every floating surface shows the desktop through it. We don't paint panel
backgrounds.

| Surface | Material |
| --- | --- |
| Dock | Liquid Glass (`NSGlassEffectView`) on macOS 26+, otherwise `NSVisualEffectMaterial::Menu` |
| Cards and tooltips | Liquid Glass, otherwise `NSVisualEffectMaterial::Popover` |
| Windows (Clipboard History, Settings, plugin windows) | `NSVisualEffectMaterial::Sidebar` behind a transparent title bar |

X11 has no system materials, so on Linux every surface is painted with the
palette's opaque surface, as with Reduce Transparency on macOS. Context
menus are drawn like `NSMenu` (`ui::menu`) in their own pop-up window.

Each platform keeps its native title bar. On macOS it is transparent and
the traffic lights sit in the window's toolbar or title row; on Windows and
Linux the system caption shows the title, so views drop the traffic-light
room and their own title row (`ui::chrome::INSET_TITLE_BAR`).

The dock and card materials stay in the active state even though Sidedoor is
never the active app. Otherwise they'd go flat and grey the moment you look at
them.

A card's material is masked to its outline, arrow included, so the glass, the
shadow and the hairline stroke all trace the same shape. Palette colors sit on
top of the material with transparency. `Palette::surface` is transparent unless
Reduce Transparency is on.

## Color

Views use `Palette` tokens only. Each token maps to an AppKit semantic color and
has a light and a dark value.

| Token | Use |
| --- | --- |
| `label` | Primary text and glyphs |
| `secondary` | Supporting text, icons in rows |
| `tertiary` | Footers, timestamps, hints |
| `separator` | Dividers between rows and above footers |
| `stroke` | Hairline around panels, buttons and keycaps |
| `fill` | Hover highlight, placeholder tiles, icon backgrounds |
| `track` | Behind gauges and meters |
| `group` | Background of a grouped form section in Settings |
| `keycap` | Face of push buttons and keycaps |
| `segment` | Selected segment of a segmented control |
| `accent_fill` | Drop-target highlight |
| `on_accent` | Text on an accent background, like a selected row |
| `blue`, `green`, `orange`, `red`, `purple` | System colors, for meaning and data |

Blue is the accent. Selected rows are filled blue with `on_accent` text, and the
text caret and selection use the system blue too. Red means destructive. Color
carries meaning, so we don't use it for decoration.

If a view needs a color that isn't in the palette, add a token with a light and
a dark value. Don't write a hex value in the view.

## Type

We use the system font at the Mac text sizes, in points, from `theme::text`.

| Style | Size | Typical use |
| --- | --- | --- |
| `DISPLAY` | 28 | The one big number on a card, like the temperature |
| `TITLE3` | 15, semibold | Card titles |
| `BODY` | 13 | Default text |
| `CALLOUT` | 12 | Row labels, meter labels, footer buttons |
| `SUBHEADLINE` | 11 | Footers and secondary lines |
| `CAPTION` | 10 | Small annotations |
| `MICRO` | 8 | Tiny labels inside tiles |

Hierarchy comes from size, weight and the `label`, `secondary` and `tertiary`
colors. We don't use all caps or letter spacing for it.

## Shape and size

Corners get tighter as things nest. A row inside a card has a smaller radius
than the card, and a button inside a row smaller still.

| Thing | Radius |
| --- | --- |
| Dock | 20 |
| Card | 16 |
| Tooltip | 7 |
| List rows, sections | 6 to 8 |
| Buttons, icon tiles | 5 to 6 |

The dock is 60 points thick, and each item gets a 52-point slot. App icons are
44 points and widget tiles 36. The dock floats 6 points off the screen edge,
with 8 points of padding at each end, centered along its edge.

Cards open away from the edge with their arrow pointing at the item, 3 points
from the dock. Built-in cards are 300 points wide, plugin cards 280 by default.
A plugin card with no fixed height fits its content, up to 600 points. Card
content sits in `card_body()`, which has 14 points of horizontal and 12 of
vertical padding. Footers go at the bottom with a separator above them.

Anything 40 points tall or less is a tooltip. It gets the tighter tooltip shape
and a smaller arrow.

## Motion

All values are in `crates/domain/src/motion.rs`.

| Motion | Duration | Character |
| --- | --- | --- |
| `DOCK_IN` | 420 ms | Quick, soft overshoot |
| `DOCK_OUT` | 220 ms | Accelerates out, no bounce |
| `ICON_IN` | 460 ms, 24 ms stagger | Springier than the dock, 16 points of travel |
| `CARD_IN` | 220 ms | Grows from 86%, a hint of overshoot |
| `CARD_MOVE` | 170 ms | Plain ease-out, never wobbles |
| `CARD_OUT` | 140 ms | Shrinks to 95% and fades |
| `WINDOW_IN` | 180 ms | Fade in |
| `REORDER` | 300 ms spring | Items part for a dragged icon and settle after the drop |

Some smaller rules:

- Clipboard rows stagger in 15 ms apart when their card opens.
- Tooltips hold a single word, so they skip the rise and do a 90 ms cross-fade.
  Their text starts part-way visible, so the pill never shows up empty.
- An icon being pressed scales to 86%.
- Something that animates on every hover, like magnification, has to be cheap
  enough to run at the display's frame rate.

## Focus and windows

- The dock and cards are borderless, non-activating panels. They take clicks
  without activating Sidedoor.
- A text field in a card takes keyboard focus without switching apps.
- Windows (Clipboard History, Settings and plugin windows) do activate the app.
  Each one records the frontmost app before it opens and reactivates that app
  when it closes.
- Opening a window that's already open brings it forward. We never open a
  second copy.
- Windows use a transparent title bar with the traffic lights centered in the
  toolbar, the sidebar material, and a 180 ms fade in.

## Interaction details

- Dragging an icon works like the Dock: it lifts out whole, its spot
  closes, and its neighbors slide apart to open a gap under the pointer.
  Dropping it lands it in the gap; dragging it off the dock puts the gap
  back where it came from.
  A plugin carries its live tile, on a piece of the dock's surface.
- Apps dragged in from Finder part the icons the same way: the dock grows a
  slot and the gap follows the pointer. Documents, apps already in the dock
  and a full dock open no gap.
- Right-click shows a context menu, as on any Mac. A plugin's own actions go at
  the top, in order.
- Menu items and buttons use Mac title case. A menu command that asks for more
  input before it acts ends in an ellipsis, like "Assign Shortcut…".
- A destructive action asks for a second click instead of showing an alert.
  The first click on "Clear History" turns it red and changes it to "Click
  Again to Clear", and the second click has to come within 3 seconds.
- A global shortcut for a widget peeks at it. The dock slides out, shows the
  card for 3 seconds, then tucks away unless the pointer arrives.
- Status text is short and specific: "Updated just now", "Updated 4 min ago".
- Errors show where they happened. A plugin that fails to compile shows the
  error in its own card, not in a dialog.

## Accessibility

| Setting | What changes |
| --- | --- |
| Reduce Motion | Nothing slides, scales, staggers or glides. The dock and cards snap into place, and content appears without fading up. Plugin `transition` props stop animating. |
| Reduce Transparency | The material is hidden, and panels paint an opaque `surface` color instead. Grouped sections get solid backgrounds. |
| Increase Contrast | `secondary`, `tertiary`, `stroke` and `separator` get stronger so text and edges hold up. |
| Light and dark mode | Every token has both values. Windows re-sync the GPUI Kit theme whenever their appearance changes. |

Check any new screen under each of these before calling it done.

## Before you ship a UI change

- Colors come from `Palette` and sizes from `theme::text`. There are no inline
  hex values or font sizes.
- Timing comes from `motion.rs`, and the thing leaves faster than it arrives.
- It doesn't take focus unless it needs the keyboard. If it does take focus, it
  gives focus back when it closes.
- It looks right in light, dark, Reduce Motion, Reduce Transparency and
  Increase Contrast.
- Put a screenshot of it next to Control Center or Finder. If it looks out of
  place, it's wrong.
