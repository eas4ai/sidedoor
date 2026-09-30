# @sidedoor/sdk cheat sheet

Condensed from the SDK guide (`sdk/README.md`) and `sdk/src/types.ts`. If
anything here disagrees with those files, they win.

## Contents

- [definePlugin](#defineplugin)
- [Settings](#settings)
- [Elements](#elements)
- [Native components](#native-components)
- [Style props](#style-props)
- [Colors](#colors)
- [Motion](#motion)
- [Hooks and state](#hooks-and-state)
- [The sidedoor object](#the-sidedoor-object)
- [Native data](#native-data)
- [Testing](#testing)

## definePlugin

```tsx
export default definePlugin({
  name: "Timer",            // literal string, shown in menus and Settings
  icon: "timer",            // literal Lucide icon name
  width: 280,               // card width in points, 120–480
  height: undefined,        // optional; default fits content up to 600
  settings: { … },          // see Settings
  card: ({ settings }) => …,  // hover card (required)
  tile: ({ settings }) => …,  // dock slot, ~44 pt square; default shows `icon`
  onClick: () => …,         // tile click; the item's shortcut clicks too
  actions: { reset: { title: "Reset", run: () => … } }, // top of context menu
  windows: {                // native windows, opened by key
    history: { title: "History", width: 480, height: 360, render: ({ settings }) => … },
  },
});
```

## Settings

`settings: { key: { title, type, description?, default?, options? } }`

| `type` | Row in Settings | Value |
| --- | --- | --- |
| `text` | text field | string |
| `secret` | hidden text field | string |
| `number` | text field | number |
| `toggle` | switch | boolean |
| `choice` | segmented control from `options` | one of `options` |

Defaults are `""`, `0`, `false` or the first option. Read settings as typed
`{ settings }` in surfaces, actions and `onClick`, with `useSetting("key")`
in deeper components, or with `sidedoor.settings()` outside render.

## Elements

| Element | Props |
| --- | --- |
| `div` | style props, `id`, `label` (VoiceOver and tests; give icon-only buttons one), `on_click`, `on_hover(hovered)`, `hover={{…}}`, `active={{…}}`, `enter={{ kind: "rise" \| "pop", duration, delay }}`, `bg_gradient={{ from, to, angle }}`, `magnify` (tile hover scaling) |
| `svg` | `path` (Lucide name), style props |
| `img` | `src` (absolute path or `https://` URL, cached 1 h, max 10 MB), `object_fit` (`contain`, `cover`, `fill`), style props |

## Native components

| Component | Props |
| --- | --- |
| `Card` | `title`, `accessory` |
| `Title`, `Text` | `variant` (`body`, `callout`, `caption`, `headline`, `title`, `display`), `secondary`, `tertiary` |
| `Icon` | `name`, `icon_size`, `color` |
| `Button` | `label`, `icon`, `variant` (`push`, `primary`, `destructive`, `link`), `disabled`, `on_click` |
| `Input` | `id` (required), `value`, `placeholder`, `secret`, `icon`, `on_change(text)`, `on_submit(text)` |
| `Switch` | `checked`, `on_change(checked)`, `disabled` |
| `Segmented` | `options`, `selected`, `on_change(index)` |
| `Slider` | `value` (0–1), `on_change(value)` while moving, `on_commit(value)` once on release, `disabled`, `color` (`accent` default); click jumps, drag moves |
| `Meter` | `label`, `fraction` (0–1), `value`, `icon`, `color`, `animated`, `value_number`, `value_suffix` |
| `ListRow` | `title`, `subtitle`, `icon`, `accessory`, `on_click` |
| `Sparkline` | `values` (0–1 each), `color` |
| `Chart` | `kind` (`line`, `area`, `bar`), `data` (`[{ label, value }]`), `color`, `name`, `x_axis`, `y_axis`, `grid`; 96 pt tall unless `h` |
| `NumberText` | `id`, `value`, `suffix`, `duration`: an animated number label |
| `Footer`, `Keycap`, `Divider`, `Spacer` | style props |

All components also take style props, applied on top.

## Style props

Named after GPUI `Styled` methods. A method without arguments is a boolean
prop. Numbers are points.

- **Flex:** `flex` `flex_col` `flex_row` `flex_wrap` `flex_1` `flex_none` `flex_grow` `flex_shrink_0` `items_*` `justify_*` `gap` `gap_x` `gap_y`
- **Size:** `size` `w` `h` `min_w` `min_h` `max_w` `max_h` (number, `"50%"`, `"full"`, `"auto"`), `size_full` `w_full` `h_full` `min_w_0`
- **Spacing:** `p` `px` `py` `pt` `pr` `pb` `pl` `m` `mx` `my` `mt` `mr` `mb` `ml` `mx_auto` `mt_auto` `ml_auto`
- **Position:** `relative` `absolute` `top` `right` `bottom` `left` `inset_0` `overflow_hidden` `overflow_y_scroll` `overflow_x_scroll`
- **Paint:** `bg` `opacity` `rounded` `rounded_full` `border` `border_1` `border_t_1` `border_b_1` `border_color` `shadow_sm` `shadow_md` `shadow_lg`
- **Text:** `text_color` `text_size` `font_weight` (`"medium"`, `"semibold"`, `"bold"`, number) `font_family` `italic` `line_height` `text_center` `text_right` `truncate` `whitespace_nowrap` `line_clamp`

A `div` with `overflow_y_scroll` scrolls when its content is taller than its
height.

## Colors

Palette tokens follow light and dark mode: `label`, `secondary`,
`tertiary`, `separator`, `stroke`, `fill`, `track`, `accent`, `on_accent`,
`blue`, `green`, `orange`, `red`, `purple`, `purple_deep`, `transparent`. Hex values
(`#rgb`, `#rrggbb`, `#rrggbbaa`) also work.

## Motion

`transition={ms}` or `transition={{ spring: true }}` on any element animates
numeric style props (size, spacing, position, `opacity`, `rounded`,
`text_size`). Reduce Motion turns it off.

## Hooks and state

| API | Use |
| --- | --- |
| `useState`, `useEffect`, `useRef`, `useMemo` | As in React |
| `useInterval(fn, ms \| null)` | Repeating timer; `null` pauses it |
| `createStore(initial)` | Shared state: `.use()` in render, `.get()` in handlers and async code, `.set(value \| fn)` anywhere |
| `useStorage(key, initial)` | Persistent JSON state, shared by key across components |
| `useSetting(key)` | One setting's typed value |
| `useCardOpen()` | Whether the card is showing |

Saving the file keeps `useState`, `useRef` and store values when they're JSON
and hook order is unchanged.

## The sidedoor object

| Call | Effect |
| --- | --- |
| `sidedoor.openUrl(url)` | Open a URL |
| `sidedoor.open(path)` | Open a file |
| `sidedoor.copy(text)` | Copy text |
| `sidedoor.notify({ title, body? })` | System notification under the plugin's name |
| `sidedoor.openWindow(key)` / `closeWindow(key)` | Show or close a declared window |
| `sidedoor.settings()` | Current settings outside render |
| `sidedoor.storage.get(key)` / `.set(key, value)` / `.delete(key)` | Persistent JSON storage outside render |
| `sidedoor.dataDir` | A folder for the plugin's own files |

`fetch`, files and timers are plain Bun.

## System data

The app doesn't hand plugins system data; get it with Bun, as the official
plugins in `plugins/` do: Weather uses `fetch` against Open-Meteo, Stats
reads `node:os`, `vm_stat` or `/proc/meminfo` and `statfsSync("/")`, and
Clipboard watches the pasteboard with a long-running `osascript` (JXA) on
macOS and `wl-paste`, `xclip` or PowerShell elsewhere.

## Testing

```tsx
import { mount } from "@sidedoor/sdk/testing";
const plugin = mount(definition, { settings?, storage? });
```

| Member | Use |
| --- | --- |
| `card`, `tile`, `window(key)` | Rendered trees |
| `text(surface?)` | Text of `"card"`, `"tile"` or `"window:<key>"` |
| `find(label \| match)` | Element by `label`/`title` prop or text, with `.click()`, `.change(v)`, `.submit(text)`; on a `Slider`, `.change(v)` also calls `on_commit` |
| `findAll(type)` | Every element of a type, e.g. `"Button"` |
| `press(label, surface?)` | Click the clickable element with that label |
| `click()`, `action(key)`, `setCardOpen(open)`, `setSettings(values)` | What the dock and Settings send |
| `closeWindow(key)` | Window close button |
| `notifications`, `storage`, `sent` | What the plugin asked for |
| `settle()` | Wait until idle: loops while re-renders and effects keep coming (a fetch → `store.set` → re-render finishes in one call), up to 100 rounds |
| `waitFor(check, { timeout?, interval? })` | Retry `check` until it stops throwing, settling between tries; throws its last error after `timeout` (1000 ms) |
| `unmount()` | Stop timers |

A plugin that throws fails the test. Use `setSystemTime` from `bun:test` for
time-based logic.
