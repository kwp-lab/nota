import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { AiDocumentReader } from "./AiDocumentReader";

afterEach(cleanup);

it("restores reading positions per version after a loading transition", () => {
  const { rerender } = render(<AiDocumentReader versionId="one" loading={false} content={null} />);
  const reader = screen.getByRole("region", { name: "AI 文档正文" });
  reader.scrollTop = 180;
  fireEvent.scroll(reader);
  rerender(<AiDocumentReader versionId="two" loading content={null} />);
  fireEvent.scroll(reader, { target: { scrollTop: 0 } });
  rerender(<AiDocumentReader versionId="two" loading={false} content={null} />);
  expect(reader.scrollTop).toBe(0);
  reader.scrollTop = 80;
  fireEvent.scroll(reader);
  rerender(<AiDocumentReader versionId="one" loading content={null} />);
  rerender(<AiDocumentReader versionId="one" loading={false} content={null} />);
  expect(reader.scrollTop).toBe(180);
});
