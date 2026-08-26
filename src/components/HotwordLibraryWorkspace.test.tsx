import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { HotwordLibraryWorkspace } from "./HotwordLibraryWorkspace";

vi.mock("../api", () => ({
  api: {
    getHotwordList: vi.fn(),
    saveHotwordList: vi.fn(),
    deleteHotwordList: vi.fn(),
  },
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("HotwordLibraryWorkspace", () => {
  it("edits one term per line, reports duplicates, and saves explicitly", async () => {
    vi.mocked(api.saveHotwordList).mockResolvedValue({
      document: {
        id: "list-1",
        name: "产品周会",
        entries: [
          { text: "Nota", weight: null },
          { text: "千问", weight: null },
        ],
        createdAt: "2026-08-25T00:00:00Z",
        updatedAt: "2026-08-25T00:00:00Z",
      },
      normalizations: [],
    });
    const onRefresh = vi.fn(async () => undefined);
    const onDirtyChange = vi.fn();
    render(
      <HotwordLibraryWorkspace
        lists={[]}
        onRefresh={onRefresh}
        onDirtyChange={onDirtyChange}
        onMessage={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /新建/ }));
    fireEvent.change(screen.getByDisplayValue("新建热词列表"), {
      target: { value: "产品周会" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "热词与短语" }), {
      target: { value: "Nota\n千问\nNota" },
    });
    expect(screen.getByText(/有效词条/).parentElement).toHaveTextContent("2");
    expect(screen.getByText(/重复项/).parentElement).toHaveTextContent("1");

    fireEvent.click(screen.getByRole("button", { name: /保存/ }));
    await waitFor(() => expect(api.saveHotwordList).toHaveBeenCalledWith({
      id: null,
      name: "产品周会",
      content: "Nota\n千问\nNota",
    }));
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("shows server normalization feedback and canonical weight syntax", async () => {
    vi.mocked(api.saveHotwordList).mockResolvedValue({
      document: {
        id: "list-2",
        name: "产品名",
        entries: [{ text: "Busabase", weight: 4 }],
        createdAt: "2026-08-26T00:00:00Z",
        updatedAt: "2026-08-26T00:00:00Z",
      },
      normalizations: [{
        line: 1,
        code: "invalidWeight",
        message: "第 1 行的权重不受支持，已替换为默认权重 4",
      }],
    });
    render(
      <HotwordLibraryWorkspace
        lists={[]}
        onRefresh={vi.fn(async () => undefined)}
        onDirtyChange={vi.fn()}
        onMessage={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /新建/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "热词与短语" }), {
      target: { value: "Busabase：10" },
    });
    fireEvent.click(screen.getByRole("button", { name: /保存/ }));

    await screen.findByText(/已替换为默认权重 4/);
    expect(screen.getByRole("textbox", { name: "热词与短语" })).toHaveValue("Busabase:4");
  });
});
