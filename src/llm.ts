import type { LlmProvider, LlmProviderKind } from "./types";

export const OPENAI_API_ROOT = "https://api.openai.com/v1";

export const isOfficialOpenAiUrl = (kind: LlmProviderKind, baseUrl: string) => {
  if (kind !== "openAi") return false;
  try {
    return new URL(baseUrl).hostname.toLowerCase() === "api.openai.com";
  } catch {
    return false;
  }
};

export const llmProviderReady = (
  provider: Pick<LlmProvider, "kind" | "baseUrl" | "hasApiKey">,
) => Boolean(provider.baseUrl.trim())
  && (!isOfficialOpenAiUrl(provider.kind, provider.baseUrl) || provider.hasApiKey);
