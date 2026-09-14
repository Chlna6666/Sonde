// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { afterEach, expect, test, vi } from "vitest";
import { CustomSelect } from "./CustomSelect";

const options = [
  { value: "auto", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
] as const;

afterEach(cleanup);

test("custom select exposes listbox semantics without a native select", () => {
  const { container } = render(<CustomSelect label="Theme" value="auto" options={options} onChange={() => undefined} />);
  const trigger = screen.getByRole("combobox", { name: "Theme" });
  expect(container.querySelector("select")).not.toBeInTheDocument();
  fireEvent.click(trigger);
  expect(screen.getByRole("listbox", { name: "Theme" })).toBeInTheDocument();
  expect(screen.getByRole("option", { name: "System" })).toHaveAttribute("aria-selected", "true");
});

test("custom select supports keyboard selection", async () => {
  const onChange = vi.fn();
  render(<CustomSelect label="Theme" value="auto" options={options} onChange={onChange} />);
  const trigger = screen.getByRole("combobox", { name: "Theme" });
  fireEvent.keyDown(trigger, { key: "ArrowDown" });
  fireEvent.keyDown(trigger, { key: "ArrowDown" });
  fireEvent.keyDown(trigger, { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("light");
  expect(trigger).toHaveAttribute("aria-expanded", "false");
});
