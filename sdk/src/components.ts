// Native components. The host draws each with the same Rust code as its own
// widgets, so a plugin looks like part of the app; style props passed here
// are applied on top.

import { jsx } from "./jsx-runtime";
import type {
  ButtonProps,
  CardProps,
  DividerProps,
  FooterProps,
  IconProps,
  InputProps,
  KeycapProps,
  ListRowProps,
  MeterProps,
  NumberTextProps,
  SegmentedProps,
  SpacerProps,
  SparklineProps,
  ChartProps,
  SwitchProps,
  TextProps,
} from "./types";

function native<P extends object>(name: string) {
  const component = (props: P) => jsx(name, props as Record<string, unknown>);
  // Named, so handler keys and errors read `Button` rather than `Component`.
  Object.defineProperty(component, "name", { value: name });
  return component;
}

/** A widget card's body: padding, and an optional title row. */
export const Card = native<CardProps>("Card");
/** Text in the card title style. */
export const Title = native<TextProps>("Title");
/** Text in one of the system text styles. */
export const Text = native<TextProps>("Text");
/** A Lucide icon. */
export const Icon = native<IconProps>("Icon");
/** A text field. The card takes keyboard focus while you type in it. */
export const Input = native<InputProps>("Input");
/** A macOS push button, or a link-style button in cards. */
export const Button = native<ButtonProps>("Button");
/** The system switch. */
export const Switch = native<SwitchProps>("Switch");
/** A segmented control, as in the Clipboard History filters. */
export const Segmented = native<SegmentedProps>("Segmented");
/** A labelled gauge, as in the Stats card. */
export const NumberText = native<NumberTextProps>("NumberText");
export const Meter = native<MeterProps>("Meter");
/** A list row with an icon, title and subtitle, as in the Clipboard card. */
export const ListRow = native<ListRowProps>("ListRow");
/** Bars of recent values, as in the Stats card. */
export const Sparkline = native<SparklineProps>("Sparkline");
/** A line, area or bar chart, 96 points tall unless `h` says otherwise. */
export const Chart = native<ChartProps>("Chart");
/** A card footer: a hairline above a row of small text or buttons. */
export const Footer = native<FooterProps>("Footer");
/** A key drawn as a keycap. */
export const Keycap = native<KeycapProps>("Keycap");
/** A hairline separator. */
export const Divider = native<DividerProps>("Divider");
/** Flexible empty space. */
export const Spacer = native<SpacerProps>("Spacer");
