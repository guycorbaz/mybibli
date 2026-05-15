# mybibli — Logo assets

## Concept

A row of book spines on a shelf, crossed by a thin red scanner laser line —
referencing the barcode-first nature of mybibli. The varied widths of the
spines also subtly echo the rhythm of an EAN-13 barcode.

## Colors

| Role             | Hex       |
| ---------------- | --------- |
| Background       | `#1a2840` |
| Spine: amber     | `#d9a55c` |
| Spine: cream     | `#ead6a8` |
| Spine: brick     | `#b85450` |
| Spine: slate     | `#5a7a8c` |
| Spine: sage      | `#7b8f6d` |
| Scanner laser    | `#e63946` |
| Shelf line       | `#ead6a8` (50% opacity) |

## Files

### Vector (SVG — recommended)

- `svg/mybibli-icon.svg` — square icon, 240×240 viewBox.
  Use for: favicon, GitHub avatar, Docker Hub avatar, app icon.
- `svg/mybibli-logo.svg` — horizontal lockup, dark wordmark.
  Use for: README header, light-themed pages.
- `svg/mybibli-logo-dark.svg` — horizontal lockup, cream wordmark.
  Use for: dark-themed pages, social media cards on dark backgrounds.

### Raster (PNG)

- `png/mybibli-icon-{16,32,48,64,128,192,256,512,1024}.png` — icon at common sizes.
  - 16/32 — favicon
  - 48 — Windows taskbar
  - 64 — small UI
  - 128/192 — PWA manifest
  - 256 — Docker Hub recommended
  - 512 — Android adaptive icon
  - 1024 — iOS App Store / high-res master
- `png/mybibli-logo-{400,800,1200}w.png` — horizontal lockup, dark wordmark.
- `png/mybibli-logo-dark-{400,800,1200}w.png` — horizontal lockup, light wordmark.
- `png/favicon.ico` — multi-size .ico (16/32/48) for browser favicons.

## HTML favicon snippet

```html
<link rel="icon" type="image/svg+xml" href="/mybibli-icon.svg">
<link rel="icon" type="image/png" sizes="32x32" href="/mybibli-icon-32.png">
<link rel="icon" type="image/png" sizes="16x16" href="/mybibli-icon-16.png">
<link rel="apple-touch-icon" sizes="192x192" href="/mybibli-icon-192.png">
<link rel="shortcut icon" href="/favicon.ico">
```

## Markdown badge for README

```markdown
![mybibli logo](docs/assets/mybibli-logo.svg)
```

## License

These assets are provided alongside the mybibli project (AGPL-3.0-or-later).
