const PDF_EXTENSION = ".pdf";
const WINDOWS_RESERVED_FILE_NAME = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i;

export function pdfDefaultFileName(title: string) {
  const sanitized = title
    .trim()
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "-")
    .replace(/[. ]+$/g, "")
    .slice(0, 120);
  const base = sanitized || "AI 文档";
  const safeBase = WINDOWS_RESERVED_FILE_NAME.test(base) ? `_${base}` : base;
  return safeBase.toLocaleLowerCase().endsWith(PDF_EXTENSION) ? safeBase : `${safeBase}${PDF_EXTENSION}`;
}

export function aiDocumentExportTitle(source: HTMLElement, fallbackTitle: string) {
  const heading = Array.from(source.querySelectorAll("h1"), (element) =>
    element.textContent?.replace(/\s+/g, " ").trim() ?? "").find(Boolean);
  return heading || fallbackTitle.trim() || "AI 文档";
}

async function waitForPrintLayout() {
  await document.fonts?.ready;
  await new Promise<void>((resolve) => {
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(() => resolve());
    } else {
      setTimeout(resolve, 0);
    }
  });
}

export async function withAiDocumentPrintRoot(
  source: HTMLElement,
  title: string,
  exportPdf: () => Promise<void>,
) {
  const previousTitle = document.title;
  const printRoot = document.createElement("article");
  printRoot.className = "ai-pdf-document";
  printRoot.setAttribute("aria-hidden", "true");

  const content = document.createElement("div");
  content.className = "ai-pdf-content";
  content.append(...Array.from(source.childNodes, (node) => node.cloneNode(true)));
  printRoot.append(content);
  document.body.append(printRoot);
  document.title = title;

  try {
    await waitForPrintLayout();
    await exportPdf();
  } finally {
    document.title = previousTitle;
    printRoot.remove();
  }
}
