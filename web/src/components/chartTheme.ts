/**
 * Shared chart theme built on theme CSS variables (tokens.css) so every
 * chart stays readable in both light and dark themes. Never hardcode
 * dark-theme hex values in chart components — use these helpers instead.
 *
 * Contract:
 * - Series/categorical colors: `chartColor(index)` (or `CHART_PALETTE`).
 * - Axis labels, tick values and tooltip numbers: theme text tokens
 *   (`var(--text)` / `var(--muted)`), never a series or brand color — a
 *   saturated chart color used as text fails contrast in the light theme.
 * - Dot/marker halos: `ACTIVE_DOT_HALO` so the dot reads on any surface.
 * - Brand identity colors (OS logos in `PlatformIcon`) are the one documented
 *   exception: they stay fixed constants, but may only be used for icon and
 *   fill surfaces, never for text.
 */

/** Theme-aware categorical palette in priority order. */
export const CHART_PALETTE = [
  "var(--signal)",
  "var(--blue)",
  "var(--amber)",
  "var(--pink)",
  "var(--purple)",
  "var(--indigo)",
  "var(--signal-strong)",
  "var(--blue-strong)",
  "var(--danger)",
] as const;

/** Pick a palette color by index, wrapping around the palette in both directions. */
export function chartColor(index: number): string {
  const size = CHART_PALETTE.length;
  return CHART_PALETTE[((index % size) + size) % size];
}

/**
 * Shorten an ISO-style datetime label for axis ticks:
 * "2026-09-14T08:00" / "2026-09-14" -> "09-14"; short labels pass through.
 */
export function shortDayLabel(label: string): string {
  if (label.length > 10) return label.slice(11);
  if (label.length > 5) return label.slice(5);
  return label;
}

/** Compact numeric formatter for axis ticks: 950 -> 950, 1200 -> 1.2k, 3M -> 3M. */
export function compactNumber(value: number): string {
  if (value >= 1_000_000) {
    const m = value / 1_000_000;
    return `${Number.isInteger(m) ? m : m.toFixed(1)}M`;
  }
  if (value >= 1_000) {
    const k = value / 1_000;
    return `${Number.isInteger(k) ? k : k.toFixed(1)}k`;
  }
  return String(value);
}

/** Theme-aware halo color for active dots / point markers. */
export const ACTIVE_DOT_HALO = "var(--bg-float)";
