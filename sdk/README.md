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

The quickest start is **Settings › Plugins › New Plugin**. It creates a
working widget, adds it to the dock and opens its code. By hand, a plugin is
a folder in `~/Library/Application Support/SidekickClone/plugins/` with an
`index.tsx`, and everything about it lives in one `definePlugin` call:

```tsx
import { Button, Card, definePlugin, useSetting, useState } from "@sidekick/sdk";

export default definePlugin({
  name: "Counter",
  icon: "hash",
  width: 260,
  settings: {
    step: { title: "Step", type: "number", default: 1 },
  },

  card() {
    const [count, setCount] = useState(0);
    const step = useSetting<number>("step");
    return (
      <Card title="Counter" accessory={`${count} clicks`}>
        <div text_size={32} font_weight="semibold">{count}</div>
        <Button variant="primary" label="Add" on_click={() => setCount(count + step)} />
      </Card>
    );
  },
});
```

- `name`: shown in the dock's menus and in Settings.
- `icon`: a [Lucide](https://lucide.dev/icons) icon name.
- `width`: the card width in points, 280 by default.
- `height`: optional. Leave it out and the card fits its content, up to 600 points.
- `settings`: optional. See [Settings](#settings).
- `card` and `tile`: see [Surfaces](#surfaces).

There is no JSON to write. The app lists a plugin by reading `name` and
`icon` from the file as text, without running it, and learns the rest when
the plugin starts.

Add the plugin to the dock under **Settings › Plugins** or **Settings ›
Items**. The app asks first, because a plugin runs with the same access as
the app: your files, the network and other programs. Only add plugins from
people you trust.

The app links `@sidekick/sdk` into the plugin's `node_modules` and adds a
`tsconfig.json` if the plugin has none, so the plugin has nothing to install.
Saving a file in the plugin reloads it. Errors show in its card, and
`console.log` output shows in its log under **Settings › Plugins**.

The app runs all plugins in one Bun process, each in its own Worker thread.
Plugins don't share state, and one that throws or hangs only stops itself.
The working directory is shared, so find your own files with
`import.meta.dir` rather than relative paths.

See [`examples/plugins/pomodoro`](examples/plugins/pomodoro) for a full widget with a custom dock tile, a setting and a fitted card.

## Surfaces

`definePlugin({ card, tile?, … })`

- `card`: the card that opens when you hover the item.
- `tile`: the dock slot, about 44 points square. Without it, the dock shows
  the plugin's `icon`.

Both are rendered all the time, not only while visible. Put timers in one
of them only. `useCardOpen()` tells you whether the card is showing, which
helps you refresh data when it opens or pause work while it's hidden.

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
| `Input` | `id` (required), `value`, `placeholder`, `secret`, `icon`, `on_change(text)`, `on_submit(text)` |
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
- **Position:** `relative` `absolute` `top` `right` `bottom` `left` `inset_0` `overflow_hidden` `overflow_y_scroll` `overflow_x_scroll`
- **Paint:** `bg` `opacity` `rounded` `rounded_full` `border` `border_1` `border_t_1` `border_b_1` `border_color` `shadow_sm` `shadow_md` `shadow_lg`
- **Text:** `text_color` `text_size` `font_weight` (`"medium"`, `"semibold"`, `"bold"` or a number) `font_family` `italic` `line_height` `text_center` `text_right` `truncate` `whitespace_nowrap` `line_clamp`

`Input` takes keyboard focus in the card without switching apps. Typing
reaches the plugin through `on_change`, and Return through `on_submit`. Set
`value` to replace the text, for example `""` to clear the field after a
submit. A `div` with `overflow_y_scroll` scrolls when its content is taller
than its own height.

Colors are palette tokens that follow light and dark mode (`label`,
`secondary`, `tertiary`, `separator`, `stroke`, `fill`, `track`, `accent`,
`on_accent`, `blue`, `green`, `orange`, `red`, `purple` and `transparent`) or
hex values (`#rrggbb` or `#rrggbbaa`).

## Motion

Add `transition` to any element, or to a native component, and its numeric
style props (sizes, spacing, position, `opacity`, `rounded` and `text_size`)
animate to new values instead of jumping:

```tsx
<div h={6} rounded_full bg="orange" w={`${progress}%`} transition={400} />
<div opacity={visible ? 1 : 0} transition={{ spring: true }} />
```

A number is a duration in milliseconds with the app's ease-out.
`{ spring: true }` uses its spring. The app draws every frame of the
animation, and Reduce Motion turns it off.

## Settings

Declare settings in `definePlugin`, by key, and **Settings › Plugins** shows
them as native rows:

| `type` | Row | Value |
| --- | --- | --- |
| `text` | text field | string |
| `secret` | hidden text field | string |
| `number` | text field | number |
| `toggle` | switch | boolean |
| `choice` | segmented control, from `options` | string |

Each setting takes `title`, `type`, and optionally `description` and
`default` (otherwise `""`, `0`, `false` or the first option). Read a setting with `useSetting("key")` during render; the widget
re-renders when it changes. Outside render, use `sidekick.settings()`.

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

The app talks to the supervisor (`src/supervisor.ts`) in JSON lines tagged
with a plugin id, and the supervisor passes messages to and from each
plugin's worker.

The plugin first sends `{"type":"manifest", …}`, with what `definePlugin`
declares. Then it sends each surface whole once:
`{"type":"render","surface":"card","tree":[…]}`, where a node is either a
string or `{"t": tag, "p": props, "c": children}`. After that it sends only
what changed: `{"type":"patch","surface":"card","patches":[{"op":"props","path":[0,2],"props":{…}}]}`,
where `op` is `replace` or `props`. A function prop travels as `{"$h": key}`.

The app sends `event` (a handler key and a value), `card` (whether the card
is open), `settings`, and `resync` if a patch doesn't fit its copy of the
tree.
