// Synchronous round trips for Rust's headless UI tests. Runs the production
// definition and bridge, so those tests click the actual TSX-built UI.
import { start, type HostMessage, type PluginMessage } from "../src/host";
import { pathToFileURL } from "node:url";

const definition = (await import(pathToFileURL(process.argv[2]!).href)).default;
let messages: PluginMessage[] = [];
let receive: (message: HostMessage) => void = () => {};
start(definition, {
  send: (message) => messages.push(message),
  listen: (handler) => {
    receive = handler;
  },
});
const flush = () => {
  process.stdout.write(`${JSON.stringify(messages)}\n`);
  messages = [];
};
flush();
for await (const line of console) {
  receive(JSON.parse(line));
  await Bun.sleep(0);
  flush();
}
