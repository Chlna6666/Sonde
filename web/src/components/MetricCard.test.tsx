// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { Activity } from "lucide-react";
import { expect, test } from "vitest";
import { MetricCard } from "./MetricCard";

test("metric card exposes its label and formatted value", () => {
  render(<MetricCard label="Events" value={1200} icon={Activity} />);
  expect(screen.getByText("Events")).toBeInTheDocument();
  expect(screen.getByText(new Intl.NumberFormat().format(1200))).toBeInTheDocument();
});
