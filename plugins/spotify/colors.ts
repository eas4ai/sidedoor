// Picks a tint from album art. macOS's `sips` shrinks the image to a 24×24
// BMP, which is simple to read without an image library, and the pixels are
// grouped by hue to find the most prominent colorful one.

import { tmpdir } from "node:os";
import { join } from "node:path";
import { rm } from "node:fs/promises";

/** Two `#rrggbb` colors for the card's gradient, top then bottom. */
export interface Tint {
  from: Hex;
  to: Hex;
  /** A brighter take on the main hue for controls; none for greyscale art. */
  accent?: Hex;
  /** Black or white, whichever reads better on `accent`. */
  onAccent?: Hex;
}

type Hex = `#${string}`;

type Rgb = [number, number, number];

/** Reads the pixels of an uncompressed 24- or 32-bit BMP, in any row order. */
export function bmpPixels(bytes: Uint8Array): Rgb[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const offset = view.getUint32(10, true);
  const width = view.getInt32(18, true);
  const height = Math.abs(view.getInt32(22, true));
  const bytesPerPixel = view.getUint16(28, true) / 8;
  if (bytesPerPixel !== 3 && bytesPerPixel !== 4) throw new Error("Unsupported BMP");
  const stride = Math.ceil((width * bytesPerPixel) / 4) * 4;
  const pixels: Rgb[] = [];
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const at = offset + y * stride + x * bytesPerPixel;
      pixels.push([bytes[at + 2]!, bytes[at + 1]!, bytes[at]!]);
    }
  }
  return pixels;
}

function toHsl([r, g, b]: Rgb): [number, number, number] {
  const [rn, gn, bn] = [r / 255, g / 255, b / 255];
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = d / (1 - Math.abs(2 * l - 1));
  const h =
    max === rn ? ((gn - bn) / d + 6) % 6 : max === gn ? (bn - rn) / d + 2 : (rn - gn) / d + 4;
  return [h * 60, s, l];
}

function toHex(h: number, s: number, l: number): Hex {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return `#${[r, g, b].map((v) => Math.round((v + m) * 255).toString(16).padStart(2, "0")).join("")}`;
}

/** Black or white, by the WCAG relative luminance of `hex`. */
export function contrastOn(hex: Hex): Hex {
  const linear = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  const luminance = 0.2126 * linear[0]! + 0.7152 * linear[1]! + 0.0722 * linear[2]!;
  // The crossover where black and white text have equal contrast.
  return luminance > 0.179 ? "#000000" : "#ffffff";
}

const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));

/**
 * The most prominent colorful hue, and a second one for the gradient's end
 * (or the same hue, darker). Lightness is kept in the middle so the
 * palette's text stays readable over it in light and dark mode.
 */
export function pickTint(pixels: Rgb[]): Tint {
  const BINS = 12;
  const bins = Array.from({ length: BINS }, () => ({ score: 0, h: 0, s: 0, l: 0, weight: 0 }));
  let total: Rgb = [0, 0, 0];
  for (const pixel of pixels) {
    total = [total[0] + pixel[0], total[1] + pixel[1], total[2] + pixel[2]];
    const [h, s, l] = toHsl(pixel);
    if (s < 0.2 || l < 0.1 || l > 0.9) continue;
    // Favour saturated pixels away from black and white.
    const weight = s * (1 - Math.abs(l - 0.5) * 1.6);
    const bin = bins[Math.floor(h / (360 / BINS)) % BINS]!;
    bin.score += weight;
    // Bins never straddle 0°, so a plain weighted mean of hue is safe.
    bin.h += h * weight;
    bin.l += l * weight;
    bin.s += s * weight;
    bin.weight += weight;
  }
  const colorOf = (bin: (typeof bins)[number]) => ({
    h: bin.h / bin.weight,
    s: bin.s / bin.weight,
    l: bin.l / bin.weight,
  });
  const ranked = bins
    .map((bin, index) => ({ bin, index }))
    .filter(({ bin }) => bin.weight > 0)
    .sort((a, b) => b.bin.score - a.bin.score);

  const [first, ...rest] = ranked;
  if (!first || first.bin.score < pixels.length * 0.02) {
    // Greyscale art: a muted version of the average.
    const [h, s, l] = toHsl(total.map((v) => v / Math.max(1, pixels.length)) as Rgb);
    return { from: toHex(h, Math.min(s, 0.12), clamp(l, 0.35, 0.5)), to: toHex(h, Math.min(s, 0.12), 0.3) };
  }
  const main = colorOf(first.bin);
  // A second hue at least three bins away and with a fair share of the art.
  const second = rest.find(
    ({ bin, index }) =>
      bin.score > first.bin.score * 0.25 &&
      Math.min(Math.abs(index - first.index), BINS - Math.abs(index - first.index)) >= 3,
  );
  const other = second ? colorOf(second.bin) : { ...main, l: main.l - 0.15 };
  const accent = toHex(main.h, clamp(main.s, 0.6, 0.9), 0.58);
  return {
    from: toHex(main.h, clamp(main.s, 0.35, 0.8), clamp(main.l, 0.4, 0.55)),
    to: toHex(other.h, clamp(other.s, 0.3, 0.75), clamp(other.l, 0.3, 0.5)),
    accent,
    onAccent: contrastOn(accent),
  };
}

/** Downloads album art and picks its tint. Tests replace it. */
export type Extractor = (artworkUrl: string) => Promise<Tint>;

let extractor: Extractor = async (artworkUrl) => {
  const response = await fetch(artworkUrl);
  if (!response.ok) throw new Error(`Artwork download failed: ${response.status}`);
  const base = join(tmpdir(), `sidedoor-spotify-${process.pid}-${Date.now()}`);
  const [image, bmp] = [`${base}.img`, `${base}.bmp`];
  try {
    await Bun.write(image, await response.arrayBuffer());
    const proc = Bun.spawn(["sips", "-s", "format", "bmp", "-z", "24", "24", image, "--out", bmp], {
      stdout: "ignore",
      stderr: "pipe",
    });
    if ((await proc.exited) !== 0) throw new Error(await new Response(proc.stderr).text());
    return pickTint(bmpPixels(new Uint8Array(await Bun.file(bmp).arrayBuffer())));
  } finally {
    await Promise.all([rm(image, { force: true }), rm(bmp, { force: true })]);
  }
};

export const setExtractor = (next: Extractor) => {
  extractor = next;
};

const cache = new Map<string, Tint>();

/** The tint for a piece of art, remembered for the session. */
export async function tintFor(artworkUrl: string): Promise<Tint> {
  const cached = cache.get(artworkUrl);
  if (cached) return cached;
  const tint = await extractor(artworkUrl);
  cache.set(artworkUrl, tint);
  return tint;
}
