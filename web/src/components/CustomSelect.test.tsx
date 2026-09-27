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

test("custom select supports numeric option values like retention days", () => {
  const numOptions = [
    { value: 30, label: "30 Days" },
    { value: 365, label: "365 Days" },
    { value: 3650, label: "3650 Days (10 Years)" },
  ] as const;
  const onChange = vi.fn();
  render(<CustomSelect label="Retention" value={3650} options={numOptions} onChange={onChange} />);
  const trigger = screen.getByRole("combobox", { name: "Retention" });
  expect(trigger).toHaveTextContent("3650 Days (10 Years)");
  fireEvent.click(trigger);
  fireEvent.click(screen.getByRole("option", { name: "30 Days" }));
  expect(onChange).toHaveBeenCalledWith(30);
});

test("custom select renders hidden input for form submission when name is provided", () => {
  const { container } = render(
    <form>
      <CustomSelect name="retentionDays" label="Retention" value={365} options={[{ value: 365, label: "1 Year" }]} onChange={() => undefined} />
    </form>
  );
  const hiddenInput = container.querySelector('input[name="retentionDays"]') as HTMLInputElement;
  expect(hiddenInput).toBeInTheDocument();
  expect(hiddenInput.value).toBe("365");
  expect(hiddenInput.type).toBe("hidden");
});

test("custom select supports placeholder when value is empty", () => {
  render(
    <CustomSelect
      label="Member"
      value=""
      placeholder="Select a member..."
      options={[{ value: "u1", label: "Alice" }]}
      onChange={() => undefined}
    />
  );
  const trigger = screen.getByRole("combobox", { name: "Member" });
  expect(trigger).toHaveTextContent("Select a member...");
});
