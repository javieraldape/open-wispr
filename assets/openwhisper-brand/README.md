# OpenWhisper — Brand Assets

Monochrome, line-art voice mark (five hollow bars) + **OpenWhisper** wordmark set in JetBrains Mono. No accent color — black on light, white on dark.

## File index

```
svg/                              vector source — open & edit in Illustrator (File ▸ Open)
  openwhisper-lockup-black.svg      horizontal logo (icon + wordmark)
  openwhisper-lockup-white.svg
  openwhisper-icon-black.svg        icon only
  openwhisper-icon-white.svg
png/
  icon/    openwhisper-icon-{black,white}-{32,64,128,256,512}.png   transparent
  lockup/  openwhisper-lockup-black-2x.png / -3x.png                transparent
           openwhisper-lockup-white-3x.png                         transparent
           openwhisper-lockup-on-white.png / -on-black.png         solid bg, drop-in
app-icon/
  ios/     openwhisper-appicon-blacktile-1024.png                  App Store / asset-catalog master (square)
           openwhisper-appicon-whitetile-1024.png
           apple-touch-icon-180.png
  pwa/     icon-192-blacktile.png, icon-512-blacktile.png
           icon-512-whitetile.png
           maskable-512-blacktile.png                              Android adaptive / maskable
favicon/
  favicon.svg                       adaptive (switches to white in dark mode)
  favicon-32.png, favicon-16.png    heavier stroke for small-size legibility
site.webmanifest
```

## Colors

| Role  | Hex       |
| ----- | --------- |
| Ink   | `#111111` |
| Paper | `#FFFFFF` |

Monochrome by design. Do not introduce an accent color.

## Typography

Wordmark: **JetBrains Mono, Bold (700)**, camelCase `OpenWhisper`, tracking `-0.03em`.
Free / open-source — https://www.jetbrains.com/lp/mono/ (also on Google Fonts).

For a font-independent final: open a lockup SVG in Illustrator and **Type ▸ Create Outlines**.

## Clear space & minimum size

- **Clear space:** keep at least one bar-width of empty space on all sides.
- **Min lockup width:** 120px (digital).
- **Min icon:** 24px — below that use `favicon/favicon-16.png` (heavier stroke).

## Do / Don't

- **Do** use black on light backgrounds, white on dark. Keep proportions. Prefer SVG.
- **Don't** recolor, fill the bars solid, add shadows/effects, stretch, or rotate.

## Implement in your app

### Web `<head>`

```html
<link rel="icon" href="/favicon/favicon.svg" type="image/svg+xml" />
<link rel="icon" href="/favicon/favicon-32.png" sizes="32x32" />
<link rel="icon" href="/favicon/favicon-16.png" sizes="16x16" />
<link rel="apple-touch-icon" href="/app-icon/ios/apple-touch-icon-180.png" />
<link rel="manifest" href="/site.webmanifest" />
<meta name="theme-color" content="#111111" />
```

### Inline SVG that inherits text color

Use `svg/openwhisper-icon-black.svg` and replace `stroke="#111111"` with `stroke="currentColor"` — the mark then takes the surrounding `color`.

### iOS (native)

Use `app-icon/ios/openwhisper-appicon-blacktile-1024.png` as the 1024 asset-catalog master. iOS applies the rounded-corner mask automatically — ship the square file.

### Android (native / PWA)

Use `app-icon/pwa/maskable-512-blacktile.png` for the adaptive (maskable) icon; the mark is inset within the safe zone.

---

Mark: five hollow bars • Wordmark: JetBrains Mono Bold • © the OpenWhisper project.
