import { estimateTokenCount } from "tokenx";
import type { AiGenerationRequestPreview } from "./types";

const asRecord = (value: unknown, label: string): Record<string, unknown> => {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${label} 不是有效的 JSON 对象`);
  }
  return value as Record<string, unknown>;
};

const asString = (value: unknown, label: string): string => {
  if (typeof value !== "string") throw new Error(`${label} 不是文本`);
  return value;
};

const estimateResponsesInput = (requestBody: unknown): number => {
  const body = asRecord(requestBody, "Responses Request Body");
  return estimateTokenCount(asString(body.instructions, "instructions"))
    + estimateTokenCount(asString(body.input, "input"));
};

const estimateChatCompletionsInput = (requestBody: unknown): number => {
  const body = asRecord(requestBody, "Chat Completions Request Body");
  if (!Array.isArray(body.messages)) throw new Error("messages 不是有效的消息数组");
  return body.messages.reduce((total, value, index) => {
    const message = asRecord(value, `messages[${index}]`);
    return total
      + estimateTokenCount(asString(message.role, `messages[${index}].role`))
      + estimateTokenCount(asString(message.content, `messages[${index}].content`));
  }, 0);
};

export const estimateAiRequestInputTokens = (
  preview: AiGenerationRequestPreview,
): number => preview.providerKind === "openAi"
  ? estimateResponsesInput(preview.requestBody)
  : estimateChatCompletionsInput(preview.requestBody);
