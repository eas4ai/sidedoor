// A history of what you copy: text, links, files and, on a Mac, images.
// Click the tile, or give it a shortcut, to search everything you copied.

import { rmSync } from "node:fs";
import {
  Card,
  Footer,
  Icon,
  Input,
  Segmented,
  Title,
  createStore,
  definePlugin,
  sidedoor,
  useEffect,
  useInterval,
  useState,
} from "@sidedoor/sdk";
import {
  EMPTY,
  FILTERS,
  age,
  day,
  images,
  promote,
  push,
  search,
  title,
  type Clip,
  type Entry,
  type Filter,
  type History,
} from "./history";
import { copyBack, watch, type Copied } from "./watch";

/** How many copies the card shows. */
const RECENT = 5;
/** How long "Click Again to Clear" waits for the second click. */
const CLEAR_ARMED_MS = 3000;

let watcher = watch;
/** Tests replace the clipboard with their own. */
export const setWatcher = (next: typeof watch) => {
  watcher = next;
};
let copier = copyBack;
export const setCopier = (next: typeof copyBack) => {
  copier = next;
};

const clips = createStore<History>(EMPTY);
const clearArmed = createStore(false);
let disarm: ReturnType<typeof setTimeout> | undefined;

function save(history: History) {
  clips.set(history);
  sidedoor.storage.set("history", history);
}

function forget(paths: string[]) {
  for (const path of paths) rmSync(path, { force: true });
}

function add({ clip, source }: Copied) {
  const { history, unused } = push(clips.get(), clip, source, Date.now());
  save(history);
  forget(unused);
}

async function copy(entry: Entry) {
  save(promote(clips.get(), entry.id, Date.now()));
  await copier(entry.clip, sidedoor.copy);
}

function requestClear() {
  if (!clearArmed.get()) {
    clearArmed.set(true);
    disarm = setTimeout(() => clearArmed.set(false), CLEAR_ARMED_MS);
    return;
  }
  clearTimeout(disarm);
  clearArmed.set(false);
  forget(images(clips.get()));
  save(EMPTY);
}

function glyph(clip: Clip): string {
  return clip.type === "link" ? "link" : clip.type === "file" ? "file" : "file-text";
}

function Thumbnail({ clip }: { clip: Clip }) {
  if (clip.type === "image")
    return <img src={clip.path} size={28} flex_shrink_0 rounded={6} object_fit="cover" />;
  return (
    <div size={28} flex_shrink_0 rounded={6} bg="fill" flex items_center justify_center>
      <Icon name={glyph(clip)} icon_size={14} color="secondary" />
    </div>
  );
}

function Row({ entry, now, onPick }: { entry: Entry; now: number; onPick: () => void }) {
  return (
    <div
      id={`clip:${entry.id}`}
      label={title(entry.clip)}
      h={46}
      flex
      items_center
      gap={10}
      px={6}
      rounded={8}
      hover={{ bg: "fill" }}
      active={{ opacity: 0.7 }}
      on_click={onPick}
    >
      <Thumbnail clip={entry.clip} />
      <div flex_1 min_w_0 flex flex_col>
        <div text_size={13} truncate>
          {title(entry.clip)}
        </div>
        <div text_size={11} text_color="secondary" truncate>
          {age(entry.copiedAt, now)}
          {entry.source ? ` · ${entry.source}` : ""}
        </div>
      </div>
    </div>
  );
}

function ClearButton() {
  const armed = clearArmed.use();
  return (
    <div
      id="clear-history"
      px={6}
      py={2}
      rounded={5}
      text_color={armed ? "red" : "blue"}
      hover={{ bg: "fill" }}
      on_click={requestClear}
    >
      {armed ? "Click Again to Clear" : "Clear History"}
    </div>
  );
}

/** Re-renders now and then, so ages like "2 min ago" stay true. */
function useNow() {
  const [now, setNow] = useState(Date.now());
  useInterval(() => setNow(Date.now()), 30_000);
  return now;
}

function HistoryWindow() {
  const history = clips.use();
  const now = useNow();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("All");
  const found = search(history, query, filter);
  const groups = (["Today", "Yesterday", "Earlier"] as const)
    .map((name) => ({ name, entries: found.filter((entry) => day(entry.copiedAt, now) === name) }))
    .filter((group) => group.entries.length > 0);
  return (
    <div flex flex_col gap={10}>
      <Input id="search" value={query} placeholder="Search" icon="search" on_change={setQuery} />
      <Segmented
        options={[...FILTERS]}
        selected={FILTERS.indexOf(filter)}
        on_change={(index: number) => setFilter(FILTERS[index] ?? "All")}
      />
      {groups.length === 0 ? (
        <div py={24} flex justify_center text_size={13} text_color="secondary">
          {history.entries.length === 0 ? "Nothing copied yet." : "No matches."}
        </div>
      ) : (
        groups.map((group) => (
          <div key={group.name} flex flex_col>
            <div px={6} pb={4} text_size={11} font_weight="semibold" text_color="secondary">
              {group.name}
            </div>
            {group.entries.map((entry) => (
              <Row
                key={entry.id}
                entry={entry}
                now={now}
                onPick={async () => {
                  await copy(entry);
                  sidedoor.closeWindow("history");
                }}
              />
            ))}
          </div>
        ))
      )}
      <Footer>
        <div>
          {history.entries.length === 1 ? "1 item" : `${history.entries.length} items`}
        </div>
        <ClearButton />
      </Footer>
    </div>
  );
}

export default definePlugin({
  name: "Clipboard",
  icon: "clipboard",
  width: 300,
  onClick: () => sidedoor.openWindow("history"),
  windows: {
    history: {
      title: "Clipboard History",
      width: 520,
      height: 520,
      render: () => <HistoryWindow />,
    },
  },
  // The tile is always rendered, so it watches the clipboard.
  tile: () => {
    useEffect(() => {
      clips.set(sidedoor.storage.get<History>("history") ?? EMPTY);
      return watcher(sidedoor.dataDir, add);
    }, []);
    const count = clips.use().entries.length;
    return (
      <div
        relative
        size={36}
        rounded={9}
        bg_gradient={{ from: "purple", to: "purple_deep", angle: 180 }}
        flex
        items_center
        justify_center
        magnify
      >
        <Icon name="clipboard" icon_size={19} color="#fff" magnify />
        {count > 0 && (
          <div
            id={`badge:${count}`}
            absolute
            right={-5}
            bottom={-4}
            min_w={16}
            h={16}
            px={4}
            rounded_full
            bg="#1a1a1ae6"
            flex
            items_center
            justify_center
            text_size={9}
            font_weight="bold"
            text_color="#fff"
            enter={{ kind: "pop", duration: 420 }}
          >
            {count > 99 ? "99+" : count}
          </div>
        )}
      </div>
    );
  },
  card: () => {
    const history = clips.use();
    const now = useNow();
    const count = history.entries.length;
    const recent = history.entries.slice(0, RECENT);
    const heading = (
      <div flex items_center justify_between>
        <Title>Clipboard</Title>
        <div text_size={12} text_color="secondary">
          {count} copied
        </div>
      </div>
    );
    if (count === 0)
      return (
        <Card h={112} gap={4}>
          {heading}
          <div text_size={12} text_color="secondary">
            Text, links, images and files you copy will appear here.
          </div>
        </Card>
      );
    return (
      <Card h={92 + recent.length * 46} px={8} gap={4}>
        <div px={6}>{heading}</div>
        <div flex flex_col>
          {recent.map((entry, index) => (
            <div key={entry.id} id={`row:${entry.id}`} enter={{ kind: "rise", duration: 240, delay: index * 15 }}>
              <Row entry={entry} now={now} onPick={() => void copy(entry)} />
            </div>
          ))}
        </div>
        <div px={6} mt_auto>
          <Footer>
            <div
              id="show-all-history"
              px={6}
              py={2}
              rounded={5}
              text_color="blue"
              hover={{ bg: "fill" }}
              on_click={() => sidedoor.openWindow("history")}
            >
              Show All
            </div>
            <ClearButton />
          </Footer>
        </div>
      </Card>
    );
  },
});
