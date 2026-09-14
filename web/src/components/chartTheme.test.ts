import { expect, test } from "vitest";

import {
  ACTIVE_DOT_HALO,
  CHART_PALETTE,
  chartColor,
  compactNumber,
  shortDayLabel,
} from "./chartTheme";

test("chart palette only exposes theme variables", () => {
  for (const color of CHART_PALETTE) {
    expect(color.startsWith("var(--")).toBe(true);
  }
  expect(ACTIVE_DOT_HALO.startsWith("var(--")).toBe(true);
});

test("chart color follows palette order and wraps around", () => {
  expect(chartColor(0)).toBe(CHART_PALETTE[0]);
  expect(chartColor(CHART_PALETTE.length - 1)).toBe(CHART_PALETTE[CHART_PALETTE.length - 1]);
  expect(chartColor(CHART_PALETTE.length)).toBe(CHART_PALETTE[0]);
  expect(chartColor(CHART_PALETTE.length + 3)).toBe(CHART_PALETTE[3]);
  expect(chartColor(-1)).toBe(CHART_PALETTE[CHART_PALETTE.length - 1]);
});

test("short day label trims dates and hourly buckets", () => {
  expect(shortDayLabel("09-14")).toBe("09-14");
  expect(shortDayLabel("2026-09-14")).toBe("09-14");
  expect(shortDayLabel("2026-09-14T08:00")).toBe("08:00");
});

test("compact number formats axis ticks", () => {
  expect(compactNumber(0)).toBe("0");
  expect(compactNumber(950)).toBe("950");
  expect(compactNumber(1_000)).toBe("1k");
  expect(compactNumber(1_200)).toBe("1.2k");
  expect(compactNumber(3_000_000)).toBe("3M");
});
