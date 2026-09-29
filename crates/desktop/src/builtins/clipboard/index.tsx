/** @jsxImportSource ../../../../../sdk/src */
import {
  Card,
  Footer,
  Icon,
  Title,
  definePlugin,
  sidedoor,
  useData,
} from "../../../../../sdk/src";

export default definePlugin({
  name: "Clipboard",
  icon: "clipboard",
  width: 300,
  data: ["clipboard"],
  onClick: () => sidedoor.clipboard.showHistory(),
  tile: () => {
    const count = useData("clipboard")?.count ?? 0;
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
    const { count, entries, clearArmed } = useData("clipboard") ?? {
      count: 0,
      entries: [],
      clearArmed: false,
    };
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
      <Card h={92 + entries.length * 46} px={8} gap={4}>
        <div px={6}>{heading}</div>
        <div flex flex_col>
          {entries.map((entry, index) => (
            <div
              key={entry.id}
              id={`row:${entry.id}`}
              enter={{ kind: "rise", duration: 240, delay: index * 15 }}
            >
              <div
                id={`clip:${entry.id}`}
                h={46}
                flex
                items_center
                gap={10}
                px={6}
                rounded={8}
                hover={{ bg: "fill" }}
                active={{ opacity: 0.7 }}
                on_click={() => sidedoor.clipboard.copyEntry(entry.id)}
              >
                {entry.kind.type === "image" ? (
                  <img
                    src={entry.kind.path}
                    size={28}
                    rounded={6}
                    object_fit="cover"
                  />
                ) : (
                  <div
                    size={28}
                    flex_shrink_0
                    rounded={6}
                    bg="fill"
                    flex
                    items_center
                    justify_center
                  >
                    <Icon
                      name={
                        entry.kind.type === "link"
                          ? "link"
                          : entry.kind.type === "file"
                            ? "file"
                            : "file-text"
                      }
                      icon_size={14}
                      color="secondary"
                    />
                  </div>
                )}
                <div flex_1 min_w_0 flex flex_col>
                  <div text_size={13} truncate>
                    {entry.title}
                  </div>
                  <div text_size={11} text_color="secondary" truncate>
                    {entry.age}
                    {entry.source ? ` · ${entry.source}` : ""}
                  </div>
                </div>
              </div>
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
              on_click={() => sidedoor.clipboard.showHistory()}
            >
              Show All
            </div>
            <div
              id="clear-history"
              px={6}
              py={2}
              rounded={5}
              text_color={clearArmed ? "red" : "blue"}
              hover={{ bg: "fill" }}
              on_click={() => sidedoor.clipboard.requestClear()}
            >
              {clearArmed ? "Click Again to Clear" : "Clear History"}
            </div>
          </Footer>
        </div>
      </Card>
    );
  },
});
