# Dioxus UI Spike Report

## Summary

**Result: PASS** — All 9 gate items validated with 22 automated Playwright tests
(desktop chromium + mobile Pixel 5). Dioxus 0.7 + Tailwind CSS v4 is viable for
the Deve Sub frontend.

## Technology stack

| Component        | Version  | Notes                                    |
|------------------|----------|------------------------------------------|
| Dioxus           | 0.7.10   | CSR mode, `dx` CLI for build/serve       |
| Tailwind CSS     | 4.3.3    | Standalone binary, `@source` scanning    |
| SSE              | EventSource | `web_sys::EventSource`, `/api/progress` endpoint |
| Rust target      | wasm32-unknown-unknown | wasm-opt SIGABRT on release; output valid |
| Playwright       | latest   | chromium headless, 2 projects            |
| Font             | DejaVu   | User-level fontconfig (no system install) |

## Gate item results

| Gate | Item                              | Tests | Result |
|------|-----------------------------------|-------|--------|
| 1    | 10,000-node virtual list          | 2     | PASS   |
| 2    | Multi-select, pagination, filter  | 4     | PASS   |
| 3    | 500-item drag-and-drop sorting    | 2     | PASS   |
| 4    | Chinese/English i18n switching    | 3     | PASS   |
| 5    | Light/dark/custom theme           | 4     | PASS   |
| 6    | SSE task progress                 | 2     | PASS   |
| 7    | 30-day traffic chart              | 1     | PASS   |
| 8    | Mobile basic operations           | 4     | PASS   |
| 9    | Playwright automated tests        | 22    | PASS   |

All 22 tests pass in ~28s (1 worker, no retries).

## Gate details

### Gate 1 — 10,000-node virtual list

- Virtual scrolling via manual scroll-position tracking (`use_signal<f64>`).
- Item height 48px, container 600px, 5-item buffer.
- Scroll-to-5000 preserves item count (>5 visible).
- Pagination info: "Page 1 of 200".

### Gate 2 — Multi-select, pagination, filtering

- Search filter: "Node-0000" narrows to 10 results (<100).
- Protocol filter: `<select>` narrows to matching protocol badges.
- Multi-select: checkbox toggles "已选 N 项" indicator.
- Pagination: Next → "Page 2", Prev → "Page 1".

### Gate 3 — 500-item drag-and-drop sorting

- 500 group items render with `draggable="true"`.
- HTML5 drag-and-drop reorders items (Group-1 → after Group-2).
- Visual feedback: dragging opacity, drop target highlight.

### Gate 4 — Chinese/English i18n switching

- `t(lang, key)` translation table, `Language` enum (Zh/En).
- Nav labels switch: 仪表盘 ↔ Dashboard, 节点管理 ↔ Nodes, etc.
- Switch persists across page navigation.

### Gate 5 — Light/dark/custom theme

- Three themes: light (default), dark (`html.dark`), amber (`html.theme-amber`).
- Theme applies `dark` or `theme-amber` class to `<html>`.
- Theme persists in `localStorage` under key `theme`.
- Flash prevention: inline `<script>` in `index.html` applies theme before paint.

### Gate 6 — SSE task progress

- Real SSE via `web_sys::EventSource` consuming `/api/progress` endpoint.
- SSE endpoint in `serve.py` streams `data: N\n\n` events 0→100% at 50ms intervals.
- Progress bar animates with CSS transition (`duration-150 ease-out`).
- Progress reaches 100% within 5s.

### Gate 7 — 30-day traffic chart

- Pure Rust SVG chart, no JS chart library.
- Upload/download paths, axis labels, gradient fills.
- SVG element renders with non-zero dimensions.

### Gate 8 — Mobile basic operations

- Pixel 5 viewport (393px).
- Hamburger "☰" button toggles sidebar (`fixed z-40`).
- Overlay (`z-30`) closes menu on click.
- Navigation via mobile menu works.
- Dashboard chart visible and width ≤ 420px on mobile.

### Gate 9 — Playwright automated tests

- 22 tests across 5 spec files.
- 2 projects: desktop chromium, mobile-chrome (Pixel 5).
- Threaded HTTP server (`ThreadingHTTPServer`) serves release build.
- Fontconfig configured with DejaVu fonts for headless rendering.

## Known issues

- `wasm-opt` crashes with SIGABRT during release optimization; WASM/JS output
  is valid and functional. Workaround: use `dx build --release` output as-is.
- `dx serve` caches/serves incomplete CSS in dev mode. Workaround: use release
  build + Python HTTP server for testing.
- No system fonts in CI environment; requires user-level fontconfig setup.

## Conclusion

Dioxus 0.7 + Tailwind CSS v4 passes all gate criteria. The technology stack is
viable for Deve Sub's frontend. No blocking issues found.
