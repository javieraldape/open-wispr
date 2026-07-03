export type OpenWisprMarkBar = Readonly<{
  x: number;
  y: number;
  h: number;
}>;

export const OPEN_WISPR_NAME = "OpenWispr";

export const OPEN_WISPR_MARK_BARS = [
  { x: 5, y: 23, h: 18 },
  { x: 17, y: 15, h: 34 },
  { x: 29, y: 7, h: 50 },
  { x: 41, y: 18, h: 28 },
  { x: 53, y: 25, h: 14 },
] as const satisfies readonly OpenWisprMarkBar[];

interface OpenWisprMarkSvgOptions {
  stroke?: string;
  strokeWidth?: number;
  background?: string;
  state?: "idle" | "recording" | "transcribing";
  includeXmlDeclaration?: boolean;
}

const escapeSvgAttribute = (value: string): string =>
  value
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

export const openWisprMarkSvg = ({
  stroke = "#111111",
  strokeWidth = 3,
  background,
  state = "idle",
  includeXmlDeclaration = true,
}: OpenWisprMarkSvgOptions = {}): string => {
  const backgroundRect = background
    ? `<rect x="0" y="0" width="64" height="64" rx="14" fill="${escapeSvgAttribute(background)}" stroke="none" />`
    : "";
  const bars = OPEN_WISPR_MARK_BARS.map(
    (bar) =>
      `<rect x="${bar.x}" y="${bar.y}" width="6" height="${bar.h}" rx="3" />`,
  ).join("");
  const stateDecoration =
    state === "recording"
      ? '<circle cx="53" cy="12" r="4" fill="currentColor" />'
      : state === "transcribing"
        ? '<path d="M49 12h8M53 8v8" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" />'
        : "";
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" fill="none" stroke="${escapeSvgAttribute(stroke)}" color="${escapeSvgAttribute(stroke)}" stroke-width="${strokeWidth}" stroke-linejoin="round">${backgroundRect}${bars}${stateDecoration}</svg>`;

  return includeXmlDeclaration
    ? `<?xml version="1.0" encoding="UTF-8"?>\n${svg}\n`
    : svg;
};
