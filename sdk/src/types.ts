// Element and style names follow GPUI's fluent API one to one, so a widget
// written here reads like the Rust it becomes: `div().flex().flex_col()
// .gap(px(8.0)).text_color(..)` is `<div flex flex_col gap={8} text_color=..>`.
// Numbers are points (GPUI's `px`).

/** Semantic colors from the host palette; they follow light and dark mode. */
export type ColorToken =
  | "label"
  | "secondary"
  | "tertiary"
  | "separator"
  | "stroke"
  | "fill"
  | "track"
  | "accent"
  | "on_accent"
  | "blue"
  | "green"
  | "orange"
  | "red"
  | "purple"
  | "transparent";

/** A palette token, or `#rgb`, `#rrggbb` or `#rrggbbaa`. */
export type Color = ColorToken | `#${string}`;

/** A size in points, a fraction such as `"50%"`, `"full"` or `"auto"`. */
export type Length = number | `${number}%` | "full" | "auto";

export type FontWeight = "normal" | "medium" | "semibold" | "bold" | number;

/**
 * Lucide icon name in kebab case, as GPUI Kit's `IconName` files are named:
 * `"timer"`, `"cloud-sun"`, `"circle-check"`, …
 */
export type IconName = string;

/** GPUI `Styled` methods, as props. Booleans are the no-argument methods. */
export interface StyleProps {
  // Display and flexbox
  flex?: boolean;
  flex_col?: boolean;
  flex_row?: boolean;
  flex_wrap?: boolean;
  flex_1?: boolean;
  flex_auto?: boolean;
  flex_none?: boolean;
  /** `true` is a factor of 1. */
  flex_grow?: boolean | number;
  flex_shrink?: boolean | number;
  flex_shrink_0?: boolean;
  items_start?: boolean;
  items_center?: boolean;
  items_end?: boolean;
  items_baseline?: boolean;
  justify_start?: boolean;
  justify_center?: boolean;
  justify_end?: boolean;
  justify_between?: boolean;
  justify_around?: boolean;
  gap?: number;
  gap_x?: number;
  gap_y?: number;

  // Size
  size?: Length;
  w?: Length;
  h?: Length;
  min_w?: Length;
  min_h?: Length;
  max_w?: Length;
  max_h?: Length;
  size_full?: boolean;
  w_full?: boolean;
  h_full?: boolean;
  min_w_0?: boolean;

  // Spacing
  p?: number;
  px?: number;
  py?: number;
  pt?: number;
  pr?: number;
  pb?: number;
  pl?: number;
  m?: number;
  mx?: number;
  my?: number;
  mt?: number;
  mr?: number;
  mb?: number;
  ml?: number;
  mx_auto?: boolean;
  mt_auto?: boolean;
  ml_auto?: boolean;

  // Position
  relative?: boolean;
  absolute?: boolean;
  top?: Length;
  right?: Length;
  bottom?: Length;
  left?: Length;
  inset_0?: boolean;
  overflow_hidden?: boolean;

  // Paint
  bg?: Color;
  opacity?: number;
  rounded?: number;
  rounded_full?: boolean;
  border?: number;
  border_1?: boolean;
  border_t_1?: boolean;
  border_b_1?: boolean;
  border_color?: Color;
  shadow_sm?: boolean;
  shadow_md?: boolean;
  shadow_lg?: boolean;

  // Text
  text_color?: Color;
  text_size?: number;
  font_weight?: FontWeight;
  font_family?: string;
  italic?: boolean;
  line_height?: number;
  text_center?: boolean;
  text_right?: boolean;
  truncate?: boolean;
  whitespace_nowrap?: boolean;
  line_clamp?: number;

  cursor_pointer?: boolean;

  /** Styles applied while the pointer is over the element. */
  hover?: StyleProps;
  /** Styles applied while the element is pressed. */
  active?: StyleProps;
}

export interface EventProps {
  /** A stable identity among siblings, like GPUI's `ElementId`. */
  id?: string | number;
  on_click?: () => void;
  on_hover?: (hovered: boolean) => void;
}

export type Child = Element | string | number | boolean | null | undefined | Child[];

export interface ChildrenProps {
  children?: Child;
}

export type Component<P = {}> = (props: P & ChildrenProps) => Child;

/** What JSX produces before it is rendered. */
export interface Element {
  type: string | Component<any>;
  props: Record<string, unknown> & ChildrenProps;
  key?: string | number;
}

/** A rendered node as it travels to the host. */
export type Node = string | { t: string; p: Record<string, unknown>; c: Node[] };

// Native components, drawn by the host exactly as its own widgets are. Each
// takes `StyleProps` too, applied on top of the native look.

export interface CardProps extends StyleProps, ChildrenProps {
  /** Heading in the card's title style. */
  title?: string;
  /** Secondary text at the end of the heading row. */
  accessory?: string;
}

export interface TextProps extends StyleProps, ChildrenProps {
  variant?: "body" | "callout" | "caption" | "headline" | "title" | "display";
  secondary?: boolean;
  tertiary?: boolean;
}

export interface IconProps extends StyleProps {
  name: IconName;
  /** Points; defaults to 14. */
  icon_size?: number;
  color?: Color;
}

export interface ButtonProps extends StyleProps, EventProps, ChildrenProps {
  label?: string;
  /** `push` is a macOS push button; `link` is plain accent text. */
  variant?: "push" | "primary" | "destructive" | "link";
  disabled?: boolean;
  icon?: IconName;
}

export interface SwitchProps extends StyleProps {
  id?: string | number;
  checked: boolean;
  on_change: (checked: boolean) => void;
  disabled?: boolean;
}

export interface SegmentedProps extends StyleProps {
  id?: string | number;
  options: string[];
  selected: number;
  on_change: (index: number) => void;
}

export interface MeterProps extends StyleProps {
  label: string;
  /** 0 to 1. */
  fraction: number;
  value?: string;
  icon?: IconName;
  color?: Color;
}

export interface ListRowProps extends StyleProps, EventProps {
  title: string;
  subtitle?: string;
  icon?: IconName;
  /** Trailing secondary text. */
  accessory?: string;
}

export interface SparklineProps extends StyleProps {
  /** 0 to 1 each. */
  values: number[];
  color?: Color;
}

export interface FooterProps extends StyleProps, ChildrenProps {}
export interface KeycapProps extends StyleProps, ChildrenProps {}
export interface DividerProps extends StyleProps {}
export interface SpacerProps extends StyleProps {}

export interface SvgProps extends StyleProps {
  /** A Lucide icon name. */
  path: IconName;
}

export interface ImgProps extends StyleProps {
  /** An absolute file path. */
  src: string;
  object_fit?: "contain" | "cover" | "fill";
}

export interface DivProps extends StyleProps, EventProps, ChildrenProps {}
