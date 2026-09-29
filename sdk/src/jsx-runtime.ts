// The automatic JSX runtime: `<div flex gap={8}>` becomes an `Element`
// description; nothing renders until the widget's surfaces are drawn.

import type {
  ChildrenProps,
  Component,
  DivProps,
  Element,
  ImgProps,
  SvgProps,
} from "./types";

export const Fragment = "fragment";

export function jsx(
  type: string | Component<any>,
  props: Record<string, unknown> & ChildrenProps,
  key?: string | number,
): Element {
  return { type, props: props ?? {}, key };
}

export const jsxs = jsx;
export const jsxDEV = jsx;

export declare namespace JSX {
  type Element = import("./types").Element;
  interface ElementChildrenAttribute {
    children: {};
  }
  interface IntrinsicAttributes {
    key?: string | number;
  }
  interface IntrinsicElements {
    div: DivProps;
    svg: SvgProps;
    img: ImgProps;
  }
}
