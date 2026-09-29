# @sidedoor/sdk

Write Sidedoor widgets in TSX. Each plugin runs under [Bun](https://bun.sh)
in its own Worker inside one shared supervisor. The app draws what it renders
with real GPUI elements, so
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
a folder in `~/Library/Application Support/Sidedoor/plugins/` with an
`index.tsx`, and everything about it lives in one `definePlugin` call:

```tsx
import { Button, Card, definePlugin, useState } from "@sidedoor/sdk";

export default definePlugin({
  name: "Counter",
  icon: "hash",
  width: 260,
  settings: {
    step: { title: "Step", type: "number", default: 1 },
  },

  card({ settings }) {
    const [count, setCount] = useState(0);
    const step = settings.step; // a number, typed from `settings` above
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
- `onClick` and `actions`: optional. See [Clicks and commands](#clicks-and-commands).
- `windows`: optional. See [Windows](#windows).

There is no JSON to write. The app lists a plugin by reading `name` and
`icon` from the file as text, without running it, and learns the rest when
the plugin starts.

Saving keeps what's on screen: `useState`, `useRef` and `createStore`
values carry over to the reloaded code, as long as they are plain JSON and
the component's hooks are in the same order.

Add the plugin to the dock with its switch under **Settings › Plugins**, or
from **Settings › Items**. The first time, the app asks, because a plugin runs
with the same access as the app: your files, the network and other programs.
Only add plugins from people you trust. Clicking a plugin's name there shows
its settings, log and code, and buttons to reload or delete it. Deleting moves
its folder to the Trash and forgets its settings and saved data.

The app links `@sidedoor/sdk` into the plugin's `node_modules` and adds a
`tsconfig.json` if the plugin has none, so the plugin has nothing to install.
Saving a file in the plugin reloads it (see above). Errors show in its card, and
`console.log` output shows in its log under **Settings › Plugins**.

The app runs all plugins in one Bun process, each in its own Worker thread.
Plugins don't share state, and one that throws or hangs only stops itself.
The working directory is shared, so find your own files with
`import.meta.dir` rather than relative paths.

See [`examples/plugins/pomodoro`](examples/plugins/pomodoro) for a full widget with a custom dock tile, a setting and a fitted card.

## Sharing a plugin

Put the plugin's folder somewhere others can reach, and they paste its link
into **Settings › Plugins › Install from URL**:

- A GitHub repository: `github.com/you/timer`, or `you/timer` for short.
- Any other Git repository, on GitLab, Codeberg, Bitbucket or your own server:
  its web link or clone URL, such as `https://git.example.com/team/timer` or
  `git@git.example.com:team/timer.git`. These are cloned with Git, so private
  repositories work when `git clone` does in Terminal. On a Mac this needs
  Apple's command line tools (`xcode-select --install`); on Windows, Git from
  git-scm.com.
- One plugin in a repository of several: the link to its folder, as the
  host shows it, like `github.com/you/widgets/tree/main/timer` or
  `gitlab.com/you/widgets/-/tree/main/timer`. A link to the repository finds
  a plugin in its top two levels of folders, and names the choices when there
  are several.
- A `.zip` or `.tar.gz` file on any web server.

The app downloads the newest version, asks whether to trust the plugin, then
installs it and puts it in the dock. **Check for Updates** under the plugin
installs a newer commit (or a changed file) in place, keeping its dock place,
settings and saved data. The app writes where a plugin came from to
`.sidedoor-source.json` in its folder.

A `package.json` with `dependencies` is installed with
`bun install --production --ignore-scripts`. `@sidedoor/sdk` needn't be listed;
the app provides it.

## Surfaces

`definePlugin({ card, tile?, … })`

Each surface is a function that gets `{ settings }`: every setting's value,
typed from the `settings` you declared, so `settings.length` in the
Pomodoro example is `"15" | "25" | "50"` and a typo is a type error.

- `card`: the card that opens when you hover the item.
- `tile`: the dock slot, about 44 points square. Without it, the dock shows
  the plugin's `icon`.

Both are rendered all the time, not only while visible. Put timers in one
of them only. `useCardOpen()` tells you whether the card is showing, which
helps you refresh data when it opens or pause work while it's hidden.

## Clicks and commands

```tsx
definePlugin({
  onClick: () => timer.toggle(),
  actions: {
    reset: { title: "Reset Timer", run: () => timer.reset() },
  },
  …
});
```

- `onClick` runs when you click the dock tile. With it, the item's global
  shortcut (**Assign Shortcut…** in its context menu) clicks too, instead
  of showing the card.
- `actions` go at the top of the item's context menu, in order.

## Windows

For more room than a card has, declare windows by key and open one with
`sidedoor.openWindow(key)`, from a button, `onClick` or an action:

```tsx
definePlugin({
  windows: {
    history: {
      title: "Focus History",
      width: 520, // points; 480 × 360 by default
      height: 400,
      render: ({ settings }) => <Chart kind="area" data={…} />,
    },
  },
  actions: {
    history: { title: "Focus History…", run: () => sidedoor.openWindow("history") },
  },
  …
});
```

The app opens a native window with the title in its title bar, draws
`render` below it with 16 points of padding, and scrolls what doesn't fit.
Opening a window that's already open brings it forward.
`sidedoor.closeWindow(key)` closes it, as does its close button, after which
the keyboard goes back to the app you were in.

A window renders only while it's open, and its component state goes when it
closes. Keep anything that should last in `useStorage` or a store. Windows
stay open when the plugin reloads.

## Elements

GPUI's elements, in lowercase:

| Element | Props |
| --- | --- |
| `div` | style props, `id`, `on_click`, `on_hover(hovered)`, `hover={{…}}`, `active={{…}}` |
| `svg` | `path` (a Lucide name), style props |
| `img` | `src` (an absolute file path or an `https://` URL), `object_fit` (`contain`, `cover` or `fill`), style props |

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
| `Chart` | `kind` (`line`, `area` or `bar`), `data` (`[{ label, value }]`), `color`, `name`, `x_axis`, `y_axis`, `grid` |
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

An `img` with a URL shows a placeholder the size of its style until the
app has downloaded it. Downloads are cached for an hour and capped at 10 MB.
A `Chart` is 96 points tall unless you give it `h`, shows a tooltip on
hover, and takes its axes and grid off by default except the labels along
the bottom.

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
`default` (otherwise `""`, `0`, `false` or the first option). Surfaces,
`onClick` and actions read them, typed, from their `{ settings }` argument,
and the widget re-renders when they change. Deeper components can use
`useSetting("key")`, and code outside render `sidedoor.settings()`.

## Hooks and state

`useState`, `useEffect`, `useRef`, `useMemo` and `useInterval(fn, ms | null)`
work as they do in React. To share state between the tile and the card, use
`createStore(initial)`: read it with `.use()` during render and change it with
`.set(value | fn)`.

`useStorage(key, initial)` works like `useState`, but the value is saved in
the plugin's data folder, so it survives reloads and restarts, and every
component that reads `key` shares it. Values must be JSON. Outside render,
use `sidedoor.storage.get(key)`, `.set(key, value)` and `.delete(key)`.

Give list items a `key` or `id` so they keep their state when the list is
reordered.

## Asking the app

- `sidedoor.openUrl(url)`: opens a URL.
- `sidedoor.open(path)`: opens a file.
- `sidedoor.copy(text)`: copies text.
- `sidedoor.openWindow(key)` and `sidedoor.closeWindow(key)`: see
  [Windows](#windows).
- `sidedoor.notify({ title, body? })`: shows a banner in Notification
  Center, under the plugin's name. macOS asks once whether Sidedoor may
  send notifications.
- `sidedoor.dataDir`: a folder the plugin can keep files in.

Everything else, such as `fetch`, files and timers, is plain Bun.

## Testing

`@sidedoor/sdk/testing` runs a plugin in `bun test` the way the app would:

```tsx
import { expect, test } from "bun:test";
import { mount } from "@sidedoor/sdk/testing";
import pomodoro from "./index";

test("the tile starts the timer", async () => {
  const plugin = mount(pomodoro, { settings: { length: "15" } });
  await plugin.click();
  expect(plugin.find("Pomodoro").props.accessory).toBe("Focusing");
});
```

`mount(definition, { settings?, storage? })` returns:

- `card` and `tile`: the rendered trees, and `text(surface?)`: their text.
  A surface is `"card"`, `"tile"` or `"window:<key>"`.
- `window(key)`: an open window's tree, and `closeWindow(key)`: its close
  button. `sidedoor.openWindow` opens windows as it would in the app.
- `find(label | match)`: an element by its `label` or `title` prop, else its
  text. It has `click()`, `change(value)` and `submit(text)`.
  `findAll(type)` lists every element of a type, e.g. `"Button"`.
- `press(label)`: clicks the clickable element with that label.
- `click()`, `action(key)`, `setCardOpen(open)` and `setSettings(values)`:
  what the dock and Settings would send.
- `notifications`, `storage` and `sent`: what the plugin asked for. Storage
  stays in memory.
- `settle()` waits for re-renders and effects; `unmount()` stops timers.

A plugin that throws fails the test. To typecheck tests, add
`"types": ["bun"]` to the plugin's `tsconfig.json` and install
`@types/bun`. See `examples/plugins/pomodoro/index.test.tsx`.

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

Open windows are surfaces too, named `window:<key>`.

The app sends `event` (a handler key and a value), `card` (whether the card
is open), `window` (a key, and whether it opened or closed), `click`,
`action` (a key from `actions`), `settings`, and `resync` if a patch
doesn't fit its copy of the tree.

## Built-in plugins and native data

Weather, Stats, and Clipboard are ordinary `definePlugin` plugins in
`crates/desktop/src/builtins/`. They ship with Sidedoor and run in the same supervisor as user
plugins. The app bundles Bun, so an installed app does not need a separate Bun
installation. Existing widget entries and their shortcuts migrate automatically.

Plugins can subscribe to the native services with `data` and read the current
value with `useData`. A value is `null` until the first update; updates rerender
the plugin automatically. Only changed values are sent.

```tsx
import { Card, Text, definePlugin, useData } from "@sidedoor/sdk";

export default definePlugin({
  name: "CPU",
  data: ["stats"],
  card: () => {
    const stats = useData("stats");
    return <Card><Text>{stats ? `${Math.round(stats.cpu)}%` : "Loading…"}</Text></Card>;
  },
});
```

- `weather`: the location selected in Sidedoor, loading/failure state, conditions,
  and hourly forecast.
- `stats`: CPU and memory percentages, storage usage, formatted capacities, and
  CPU history. Rust samples these every two seconds.
- `clipboard`: the total count, five latest entries, and clear-confirmation state.
  Declaring this feed also enables `sidedoor.clipboard.copyEntry(id)`,
  `showHistory()`, and `requestClear()`. Clearing retains the native two-click
  confirmation. The full searchable history window remains native.

`NumberText` eases numeric labels on the native animation clock. `Meter` accepts
`animated`, `value_number`, and `value_suffix` for the same stats animations.
Tile nodes can opt into hover scaling with `magnify`; `div` supports
`enter={{ kind: "rise", duration: 240, delay: 15 }}` (or `kind: "pop"`), and
`bg_gradient={{ from: "purple", to: "purple_deep", angle: 180 }}`. Native motion
respects Reduce Motion.

For development, run `bun install --frozen-lockfile`, `bun run check`, and
`bun test` from `sdk/`, then `cargo test --workspace --locked` from the repository root.
The Rust UI tests use Bun to render the actual built-in TSX against fake native
services. `scripts/package/macos.sh` packages the runtime and compiles the built-ins.
