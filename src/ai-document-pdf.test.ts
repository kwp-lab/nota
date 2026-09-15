import { describe, expect, it, vi } from "vitest";
import {
  aiDocumentExportTitle,
  pdfDefaultFileName,
  withAiDocumentPrintRoot,
} from "./ai-document-pdf";

describe("AI document PDF helpers", () => {
  it("creates a safe PDF filename without duplicating the extension", () => {
    expect(pdfDefaultFileName("  Weekly: review?  ")).toBe("Weekly- review-.pdf");
    expect(pdfDefaultFileName("Notes.PDF")).toBe("Notes.PDF");
    expect(pdfDefaultFileName("<>. ")).toBe("--.pdf");
    expect(pdfDefaultFileName("   ")).toBe("AI 文档.pdf");
  });

  it("avoids Windows reserved device names", () => {
    expect(pdfDefaultFileName("CON")).toBe("_CON.pdf");
    expect(pdfDefaultFileName("LPT9.pdf")).toBe("_LPT9.pdf");
  });

  it("uses the first Markdown H1 as the export title", () => {
    const source = document.createElement("div");
    source.innerHTML = "<p>Intro</p><h1> </h1><h1>  Weekly <em>product</em> review  </h1><h1>Ignored</h1>";
    expect(aiDocumentExportTitle(source, "Stored title")).toBe("Weekly product review");
    source.querySelectorAll("h1").forEach((heading) => heading.remove());
    expect(aiDocumentExportTitle(source, " Stored title ")).toBe("Stored title");
  });

  it("removes the cloned print root even when export fails", async () => {
    const source = document.createElement("div");
    source.innerHTML = "<h1>Printable</h1><p>Body</p>";
    const exportPdf = vi.fn(async () => {
      expect(document.querySelector(".ai-pdf-document h1")).toHaveTextContent("Printable");
      throw new Error("synthetic export failure");
    });

    const originalTitle = document.title;
    await expect(withAiDocumentPrintRoot(source, "Printable title", exportPdf)).rejects.toThrow("synthetic export failure");
    expect(exportPdf).toHaveBeenCalledOnce();
    expect(document.querySelector(".ai-pdf-document")).toBeNull();
    expect(document.title).toBe(originalTitle);
  });
});
