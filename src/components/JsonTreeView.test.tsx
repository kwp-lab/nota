import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { JsonTreeView } from "./JsonTreeView";

afterEach(cleanup);

it("preserves pointer and keyboard expansion with the Lucide mask style hooks", () => {
  render(<JsonTreeView data={{ nested: { value: "example" } }} ariaLabel="JSON example" />);
  const toggle = screen.getByRole("button", { name: "展开 JSON 节点" });
  expect(toggle).toHaveClass("app-json-tree-expand");
  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(toggle).toHaveClass("app-json-tree-collapse");
  expect(screen.getByText('"example"')).toBeInTheDocument();
  fireEvent.keyDown(toggle, { key: "ArrowLeft" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText('"example"')).not.toBeInTheDocument();
  fireEvent.keyDown(toggle, { key: "ArrowRight" });
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(toggle).toHaveFocus();
});
