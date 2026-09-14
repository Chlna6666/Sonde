# Web theming and chart colors

Sonde ships one theme with a dark and a light variant. Both are defined as CSS custom
properties in `web/src/styles/tokens.css` (`:root` = dark, `[data-theme="light"]` = light), and
`web/src/lib/theme.ts` switches the attribute while keeping the `theme-color` meta tag in sync.

## Rules

1. **Style with tokens, not literals.** Use `var(--…)` (or the matching Tailwind arbitrary
   value such as `bg-[var(--panel)]`) in components. A literal hex value only belongs in
   `tokens.css`, where it defines a token for both themes.
2. **Chart palette comes from `chartTheme.ts`.** Series and categorical colors must come from
   `chartColor(index)` / `CHART_PALETTE`, which resolve to theme tokens. Every chart in the
   app (spline area, multi-line, donut, build bars) follows this so a theme switch never
   leaves an unreadable series.
3. **Text is never a chart color.** Axis ticks, tooltip numbers and legend values use text
   tokens (`var(--text)` / `var(--muted)`). Saturated series colors used as text fail contrast
   on the light variant (for example `#38bdf8` on `#fffdf8` is roughly 2:1).
4. **Active dots use `ACTIVE_DOT_HALO`** (`var(--bg-float)`) as their stroke so the marker
   reads against whatever surface the chart sits on.
5. **Brand identity colors are the single exception.** `PlatformIcon.getPlatformColor` returns
   fixed brand colors for OS families (Ubuntu orange, Debian red, …) because they are
   recognizable marks. They may be used for icons and fill surfaces — **never for text**.
6. **Element resets live in `@layer base`.** `tokens.css` keeps its `button`, `input`, `a`
   resets inside `@layer base` so Tailwind utilities (for instance
   `text-[var(--signal-ink)]` on a button) keep winning the cascade.

## Adding a chart color

1. Prefer an existing token in `CHART_PALETTE`.
2. If a new hue is genuinely needed, add the token to both theme blocks in `tokens.css` (dark
   and light) and append it to `CHART_PALETTE` — never inline a hex value in a component.
3. `web/src/components/chartTheme.test.ts` asserts that the palette only ever exposes
   `var(--…)` entries and that compaction/label helpers keep their contract.
