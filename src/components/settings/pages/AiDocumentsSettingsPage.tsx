import { Bot, FileText, Folder } from "lucide-react";
import { llmProviderReady } from "../../../llm";
import type { SettingsController } from "../hooks/useSettingsController";
import { SelectControl, SettingsCard, SettingsPageHeader, SettingsSection } from "../SettingsPrimitives";
import type { SettingsModel } from "../types";

export function AiDocumentsSettingsPage(props: {
  model: SettingsModel;
  controller: SettingsController;
  onManageProviders: () => void;
  onManageTemplates: () => void;
}) {
  const activeProvider = props.model.llmProviders.find(
    (provider) => provider.id === props.model.settings.activeLlmProviderId,
  );

  return (
    <div className="settings-preference-page">
      <SettingsPageHeader
        title="AI 文档"
        description="配置 Markdown 生成所用的模型服务和模板。"
      />
      <SettingsSection title="默认行为">
        <SettingsCard
          icon={<Bot size={17} />}
          title="默认 LLM Provider"
          description={activeProvider ? `${activeProvider.modelId} · ${activeProvider.baseUrl}` : "未选择"}
        >
          <SelectControl
            aria-label="默认 LLM Provider"
            value={props.model.settings.activeLlmProviderId ?? ""}
            onChange={(event) => void props.controller
              .setActiveLlmProvider(event.target.value || null)
              .catch(() => undefined)}
          >
            <option value="">未选择</option>
            {props.model.llmProviders.map((provider) => (
              <option
                key={provider.id}
                value={provider.id}
                disabled={!llmProviderReady(provider)}
              >
                {provider.name}{llmProviderReady(provider) ? "" : "（缺少 API Key）"}
              </option>
            ))}
          </SelectControl>
        </SettingsCard>
        <SettingsCard
          icon={<Folder size={17} />}
          title="Markdown 保存目录"
          description={props.model.settings.aiDocumentsDirectory}
          className="settings-path-card"
        >
          <button
            type="button"
            className="button secondary"
            onClick={() => void props.controller.chooseAiDocumentsDirectory().catch(() => undefined)}
          >
            更改
          </button>
        </SettingsCard>
      </SettingsSection>
      <SettingsSection title="管理">
        <SettingsCard
          icon={<Bot size={17} />}
          title="管理模型服务"
          description={`${props.model.llmProviders.length} 个 Provider`}
          onClick={props.onManageProviders}
        />
        <SettingsCard
          icon={<FileText size={17} />}
          title="管理 AI 模板"
          description="查看内置模板，创建和维护自定义模板"
          onClick={props.onManageTemplates}
        />
      </SettingsSection>
      <div className="settings-privacy-note">
        <FileText size={17} />
        <span>只有手动生成 AI 文档时，转写内容才会发送到所选模型服务。</span>
      </div>
    </div>
  );
}
