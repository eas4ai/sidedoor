---
name: sidedoor-plugins
description: Build, debug, test and share Sidedoor plugins, the TSX widgets that run in the Sidedoor dock via the @sidedoor/sdk package. Use when creating or editing a plugin's index.tsx, a definePlugin call, a dock card, tile, window, action or setting, when using useStorage, createStore, useData or sidedoor.* APIs, when writing bun tests with @sidedoor/sdk/testing, or when a plugin won't load, render or reload.
---

# Sidedoor plugins

A Sidedoor plugin is a folder with an `index.tsx` whose default export is a
`definePlugin({...})` call. The app runs every plugin under Bun, each in its
own Worker, and draws what it renders with native GPUI elements. There is no
DOM, HTML or CSS. Saving a file reloads the plugin in the running app.

## 1. Find or create the plugin folder

Plugins live in the `plugins` folder of the app's data directory:

| OS | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/Sidedoor/plugins/<id>/` |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/sidedoor/plugins/<id>/` |
| Windows | `%APPDATA%\Sidedoor\plugins\<id>\` |

The folder name is the plugin's id. The quickest way to start is
**Settings › Plugins › New Plugin**: it creates a working plugin, adds it to
the dock and opens its code. To start by hand, create the folder with
`index.tsx` (use the template below). The app links `@sidedoor/sdk` into its
`node_modules` and writes a `tsconfig.json` the first time it loads the
plugin. Plugins in a git repository elsewhere work too, as long as that repo
is what users install from (see step 6).

## 2. Read the API before writing code

The SDK is the source of truth. Read these rather than guessing names:

1. `node_modules/@sidedoor/sdk/README.md` (full guide), if present. On
   Windows, and when it's missing, use
   https://github.com/lassejlv/sidedoor/blob/main/sdk/README.md.
2. `node_modules/@sidedoor/sdk/src/types.ts`: every element, component and
   style prop, with types.
3. `node_modules/@sidedoor/sdk/src/data.ts`: the shapes of native data feeds.

[references/api.md](references/api.md) is a condensed cheat sheet of the
same API. [references/example.md](references/example.md) is a complete plugin
with a tile, a card, a setting, storage, an action, a window and tests.

## 3. Write the plugin

Start from this template:

```tsx
import { Button, Card, Text, definePlugin, useStorage } from "@sidedoor/sdk";

export default definePlugin({
  name: "Counter",
  icon: "hash",
  width: 260,
  settings: {
    step: { title: "Step", type: "number", default: 1 },
  },
  card({ settings }) {
    const [count, setCount] = useStorage("count", 0);
    return (
      <Card title="Counter" accessory={`${count} clicks`}>
        <Text variant="display">{count}</Text>
        <Button variant="primary" label="Add" on_click={() => setCount(count + settings.step)} />
      </Card>
    );
  },
});
```

Rules that aren't obvious from React experience:

- **`name` and `icon` come first, as plain string literals.** The app reads
  them from the file as text, without running it, to list the plugin. It
  takes the first `name:`/`icon:` after `definePlugin(`, so a variable, a
  template with `${}`, or a setting called `name` placed above it breaks the
  listing. `icon` is a [Lucide](https://lucide.dev/icons) name.
- **GPUI names, not HTML or CSS.** The only elements are `div`, `svg` and
  `img`. Style with boolean or value props named after GPUI's `Styled`
  methods, in snake_case: `<div flex flex_col gap={8} px={12} text_color="secondary">`.
  Events are `on_click`, `on_hover`, `on_change`, `on_submit` and (on
  `Slider`) `on_commit`. There's no
  `className`, `style` or `onClick`. Numbers are points.
- **Prefer native components** (`Card`, `Text`, `Button`, `ListRow`,
  `Meter`, `Chart`, …) so the widget matches the built-ins. Style props on
  them apply on top.
- **Colors are palette tokens** that follow light and dark mode: `label`,
  `secondary`, `tertiary`, `separator`, `fill`, `track`, `accent`, `blue`,
  `green`, `orange`, `red`, `purple`, … Use hex (`#rrggbb`) only when a
  token doesn't fit.
- **`card` and `tile` always render,** not only while visible. Run a timer
  in one of them only. Pause work with `useInterval(fn, null)` and use
  `useCardOpen()` to refresh data when the card opens.
- **State:** `useState` is per component. `createStore(initial)` shares state
  between tile, card and windows (`.use()` in render, `.get()` in handlers
  and async code, `.set()` anywhere).
  `useStorage(key, initial)` persists JSON across reloads and restarts.
  Outside render, use `sidedoor.storage.get/set/delete`.
- **Windows** (`windows: { key: { title, render } }`, opened with
  `sidedoor.openWindow(key)`) lose component state when closed. Keep what
  matters in storage or a store.
- **Native data:** declare `data: ["weather" | "stats" | "clipboard"]` and
  read it with `useData(name)`, which is `null` until the first update.
- **`Input` needs an `id`.** Give list items a `key`.
- **Seek bars and volume:** use `Slider` (`value` 0–1, `on_change` while
  dragging, `on_commit` on release). Send costly commands from `on_commit`.
- **Icon-only buttons:** give the clickable `div` a `label` (`label="Play"`)
  so VoiceOver reads it and tests can `press("Play")`.
- **Everything else is plain Bun:** `fetch`, `Bun.file`, timers. Find your
  own files with `import.meta.dir`, since the working directory is shared.
  Write files to `sidedoor.dataDir`.
- **Size:** `width` is clamped to 120–480 points (280 by default). Leave out
  `height` so the card fits its content, up to 600.
- A plugin runs with the app's full access. Don't read or send user data the
  widget doesn't need.

## 4. Check it

From the plugin folder:

```bash
bunx -p typescript@7.0.2 tsc --noEmit -p .
bun test
```

Pin TypeScript to the version in `node_modules/@sidedoor/sdk/package.json`;
a plain `bunx tsc` fetches an unrelated package, and newer TypeScript
releases can reject the SDK's types. Settings are typed from the `settings`
you declare, so a wrong key or value is a type error. Write tests with `@sidedoor/sdk/testing`:

```tsx
import { expect, test } from "bun:test";
import { mount } from "@sidedoor/sdk/testing";
import plugin from "./index";

test("Add counts by the step", async () => {
  const p = mount(plugin, { settings: { step: 2 } });
  await p.press("Add");
  expect(p.find("Counter").props.accessory).toBe("2 clicks");
  p.unmount();
});
```

For typechecking tests, give the plugin Bun's types from its folder:

```bash
echo '{ "private": true }' > package.json
bun add -d @types/bun
```

Then add `"types": ["bun"]` to `compilerOptions` in `tsconfig.json`.
TypeScript 7 includes no `@types` package unless `types` lists it, and the
linked SDK doesn't provide Bun's types. Create `package.json` first: without
it, `bun add` installs into the nearest parent folder with one (often the
home folder), and `tsc` then passes only by accident. Use `plugin.waitFor(fn)`
for timer or slow async work, and `plugin.settle()` otherwise. The `mount`
API is summarized in
[references/api.md](references/api.md#testing).

## 5. Run it in the app

Saving reloads the plugin, and `useState`, `useRef` and store values carry
over when they're JSON. The first time, turn the plugin on under
**Settings › Plugins** and confirm the trust prompt. Errors show in its
card. `console.log` output and the reload button are under the plugin's
name in **Settings › Plugins**.

If it doesn't show up, check in this order:

1. The folder is directly inside `plugins/` and holds `index.tsx`, `.ts`,
   `.jsx` or `.js`.
2. The file calls `definePlugin(` and default-exports it.
3. `name` and `icon` are string literals.
4. `bunx -p typescript@7.0.2 tsc --noEmit -p .` passes.

## 6. Share it

Push the folder to a repository or host an archive. Users paste the link
into **Settings › Plugins › Install from URL**. The link can be:

- a GitHub repo (`you/timer`)
- any Git URL
- a folder inside a repo (`github.com/you/widgets/tree/main/timer`)
- a `.zip` or `.tar.gz` file

Runtime dependencies go in `package.json` `dependencies`. They're installed
with `bun install --production --ignore-scripts`, so don't rely on install
scripts. Never list `@sidedoor/sdk`; the app provides it. **Check for
Updates** in Settings installs newer commits in place.

## Done checklist

- [ ] `export default definePlugin({ name: "…", icon: "…", … })` with literal name and icon first.
- [ ] Only `div`/`svg`/`img` plus SDK components; snake_case GPUI style props; palette colors.
- [ ] One timer at most, paused when idle; persistent state in `useStorage` or `sidedoor.storage`.
- [ ] `bunx -p typescript@7.0.2 tsc --noEmit -p .` and `bun test` pass.
- [ ] Reloaded in the app without errors in the card or log.
