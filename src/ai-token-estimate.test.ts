import { estimateTokenCount } from "tokenx";
import { describe, expect, it } from "vitest";
import { estimateAiRequestInputTokens } from "./ai-token-estimate";

describe("AI request input-token estimation", () => {
  it("counts only model-visible Responses input fields", () => {
    const preview = {
      providerKind: "openAi" as const,
      requestBody: {
        model: "gpt-test",
        instructions: "Follow the policy.",
        input: "Summarize this meeting.",
        max_output_tokens: 4_096,
        store: false,
      },
    };
    const expected = estimateTokenCount("Follow the policy.")
      + estimateTokenCount("Summarize this meeting.");
    expect(estimateAiRequestInputTokens(preview)).toBe(expected);
    expect(estimateAiRequestInputTokens({
      ...preview,
      requestBody: { ...preview.requestBody, model: "ignored", max_output_tokens: 1 },
    })).toBe(expected);
  });

  it("counts every Chat Completions role and message body", () => {
    const preview = {
      providerKind: "openAiCompatible" as const,
      requestBody: {
        model: "compatible-test",
        messages: [
          { role: "system", content: "Follow the policy." },
          { role: "user", content: "总结这场会议。" },
        ],
        max_tokens: 4_096,
        stream: false,
      },
    };
    const expected = ["system", "Follow the policy.", "user", "总结这场会议。"]
      .reduce((total, value) => total + estimateTokenCount(value), 0);
    expect(estimateAiRequestInputTokens(preview)).toBe(expected);
  });

  it("rejects a preview whose input shape does not match its provider", () => {
    expect(() => estimateAiRequestInputTokens({
      providerKind: "openAiCompatible",
      requestBody: { messages: [{ role: "user" }] },
    })).toThrow("messages[0].content 不是文本");
  });
});
