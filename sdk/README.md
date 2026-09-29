# @sidekick/sdk

Write Sidekick Clone widgets in TSX. Each plugin runs under [Bun](https://bun.sh)
in its own process. The app draws what it renders with real GPUI elements, so
there's no web view. Element and style names are GPUI's own, so markup moves
into the Rust UI with almost no changes:

```tsx
<div flex flex_col gap={8} px={12} text_color="secondary">…</div>
```

```rust
div().flex().flex_col().gap(px(8.0)).px(px(12.0)).text_color(palette.secondary)
```

## A plugin

A plugin is a folder in `~/Library/Application Support/SidekickClone/plugins/`:

```
plugins/pomodoro/
  package.json
  index.tsx
```

```json
{
  "name": "pomodoro",
  "main": "index.tsx",
  "sidekick": { "name": "Pomodoro", "icon": "timer", "width": 290, "height": 176 }
}
```

`icon` is a [Lucide](https://lucide.dev/icons) name. `width` and `height` set the
card size in points. Add the plugin to the dock in **Settings › Items**.

```tsx
import { Button, Card, useState, widget } from "@sidekick/sdk";

export default widget({
  card() {
    const [count, setCount] = useState(0);
    return (
      <Card title="Counter" accessory={`${count} clicks`}>
        <div text_size={32} font_weight="semibold">{count}</div>
        <Button variant="primary" label="Add" on_click={() => setCount(count + 1)} />
      </Card>
    );
  },
});
```

The app links `@sidekick/sdk` into the plugin's `node_modules` and adds a
`tsconfig.json` if the plugin has none, so the plugin has nothing to install.
Saving a file in the plugin reloads it, and errors show in its card. Anything logged with `console.log` goes to the app's stderr.

See `examples/plugins/pomodoro` for a full widget with a custom dock tile.

## Surfaces

`widget({ card, tile? })`

- `card`: the card that opens when you hover the item.
- `tile`: the dock slot, about 44 points square. Without it, the dock shows
  the manifest's icon.

Both are rendered all the time, not only while visible. Put timers in one
of them only.

## Elements

GPUI's elements, in lowercase:

| Element | Props |
| --- | --- |
| `div` | style props, `id`, `on_click`, `on_hover(hovered)`, `hover={{…}}`, `active={{…}}` |
| `svg` | `path` (a Lucide name), style props |
| `img` | `src` (an absolute file path), `object_fit` (`contain`, `cover` or `fill`), style props |

## Native components

These are drawn by the app's own Rust code, so they match the built-in
widgets. They also take style props, which are applied on top.

| Component | Props |
| --- | --- |
| `Card` | `title`, `accessory`: the padded widget body with a title row |
| `Title`, `Text` | `variant` (`body`, `callout`, `caption`, `headline`, `title` or `display`), `secondary`, `tertiary` |
| `Icon` | `name`, `icon_size`, `color` |
| `Button` | `label`, `icon`, `variant` (`push`, `primary`, `destructive` or `link`), `disabled`, `on_click` |
| `Switch` | `checked`, `on_change(checked)`, `disabled` |
| `Segmented` | `options`, `selected`, `on_change(index)` |
| `Meter` | `label`, `fraction` (0–1), `value`, `icon`, `color` |
| `ListRow` | `title`, `subtitle`, `icon`, `accessory`, `on_click` |
| `Sparkline` | `values` (0–1 each), `color` |
| `Footer`, `Keycap`, `Divider`, `Spacer` | style props |

## Style props

These are named after GPUI's `Styled` methods. A method with no arguments
becomes a boolean prop, and numbers are points.

- **Flex:** `flex` `flex_col` `flex_row` `flex_wrap` `flex_1` `flex_none` `flex_grow` `flex_shrink_0` `items_*` `justify_*` `gap` `gap_x` `gap_y`
- **Size:** `size` `w` `h` `min_w` `min_h` `max_w` `max_h`, which take a number, `"50%"`, `"full"` or `"auto"`, plus `size_full` `w_full` `h_full` `min_w_0`
- **Spacing:** `p` `px` `py` `pt` `pr` `pb` `pl` `m` `mx` `my` `mt` `mr` `mb` `ml` `mx_auto` `mt_auto` `ml_auto`
- **Position:** `relative` `absolute` `top` `right` `bottom` `left` `inset_0` `overflow_hidden`
- **Paint:** `bg` `opacity` `rounded` `rounded_full` `border` `border_1` `border_t_1` `border_b_1` `border_color` `shadow_sm` `shadow_md` `shadow_lg`
- **Text:** `text_color` `text_size` `font_weight` (`"medium"`, `"semibold"`, `"bold"` or a number) `font_family` `italic` `line_height` `text_center` `text_right` `truncate` `whitespace_nowrap` `line_clamp`

Colors are palette tokens that follow light and dark mode (`label`,
`secondary`, `tertiary`, `separator`, `stroke`, `fill`, `track`, `accent`,
`on_accent`, `blue`, `green`, `orange`, `red`, `purple` and `transparent`) or
hex values (`#rrggbb` or `#rrggbbaa`).

## Hooks and state

`useState`, `useEffect`, `useRef`, `useMemo` and `useInterval(fn, ms | null)`
work as they do in React. To share state between the tile and the card, use
`createStore(initial)`: read it with `.use()` during render and change it with
`.set(value | fn)`.

Give list items a `key` or `id` so they keep their state when the list is
reordered.

## Asking the app

- `sidekick.openUrl(url)`: opens a URL.
- `sidekick.open(path)`: opens a file.
- `sidekick.copy(text)`: copies text.
- `sidekick.dataDir`: a folder the plugin can keep files in.

Everything else, such as `fetch`, files and timers, is plain Bun.

## Protocol

The app and a plugin exchange JSON lines. On stdout the plugin sends
`{"type":"render","surface":"card","tree":[…]}`, where a node is either a
string or `{"t": tag, "p": props, "c": children}`. A function prop travels as
`{"$h": key}`. On stdin the app sends
`{"type":"event","handler":key,"value":…}` back.
