// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { Button } from "./Button";
import { Badge } from "./Badge";
import { Card, CardHeader, CardTitle, CardContent } from "./Card";
import { Input } from "./Input";

describe("UI Components", () => {
  it("renders Button with variant and loading state", () => {
    const { rerender } = render(<Button variant="default">Submit</Button>);
    expect(screen.getByRole("button", { name: /submit/i })).toBeInTheDocument();

    rerender(<Button loading>Submit</Button>);
    expect(screen.getByRole("button")).toBeDisabled();
  });

  it("renders Badge with variant and dot", () => {
    render(<Badge variant="success" dot>Online</Badge>);
    expect(screen.getByText("Online")).toBeInTheDocument();
  });

  it("renders Card structure", () => {
    render(
      <Card>
        <CardHeader>
          <CardTitle>System Overview</CardTitle>
        </CardHeader>
        <CardContent>All systems nominal</CardContent>
      </Card>
    );
    expect(screen.getByText("System Overview")).toBeInTheDocument();
    expect(screen.getByText("All systems nominal")).toBeInTheDocument();
  });

  it("renders Input with error state", () => {
    render(<Input placeholder="Enter username" error />);
    const input = screen.getByPlaceholderText("Enter username");
    expect(input).toHaveClass("border-[var(--danger)]");
  });
});
