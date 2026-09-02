import { Bot, ShieldCheck, Wifi } from "lucide-react";
import type { SettingsController } from "../hooks/useSettingsController";
import { SelectControl, SettingsCard, SettingsPageHeader, SettingsSection } from "../SettingsPrimitives";
import type { SettingsModel } from "../types";

export function TranscriptionSettingsPage(props: {
  model: SettingsModel;
  controller: SettingsController;
  onManageProviders: () => void;
}) {
  const activeProvider = props.model.asrProviders.find(
    (provider) => provider.id === props.model.settings.activeAsrProviderId,
  );

  return (
    <div className="settings-preference-page">
      <SettingsPageHeader
        title="语音转写"
        description="选择默认转写行为与服务。"
      />
      <SettingsSection title="默认行为">
        <SettingsCard
          icon={<Bot size={17} />}
          title="默认转写服务"
          description={activeProvider
            ? `${activeProvider.baseUrl} · ${activeProvider.modelId}`
            : "未选择服务时仍可正常录音"}
        >
          <SelectControl
            aria-label="默认语音转写服务"
            value={props.model.settings.activeAsrProviderId ?? ""}
            onChange={(event) => void props.controller
              .setActiveAsrProvider(event.target.value || null)
              .catch(() => undefined)}
          >
            <option value="">未选择</option>
            {props.model.asrProviders.map((provider) => (
              <option key={provider.id} value={provider.id}>{provider.name}</option>
            ))}
          </SelectControl>
        </SettingsCard>
        <SettingsCard
          icon={<Wifi size={17} />}
          title="录音结束后自动转写"
          description={activeProvider?.kind === "dashScope"
            ? "停止并保存后会上传完整录音至阿里云；录音进行中不会上传。"
            : "仅在停止并保存后开始；录音进行中不会上传。"}
        >
          <input
            type="checkbox"
            aria-label="录音结束后自动转写"
            disabled={!activeProvider}
            checked={props.model.settings.autoTranscribe}
            onChange={(event) => void props.controller
              .savePatch({ autoTranscribe: event.target.checked })
              .catch(() => undefined)}
          />
        </SettingsCard>
      </SettingsSection>
      <SettingsSection title="服务">
        <SettingsCard
          icon={<Bot size={17} />}
          title="管理转写服务"
          description={`${props.model.asrProviders.length} 个服务${activeProvider ? ` · ${activeProvider.name} 当前启用` : ""}`}
          onClick={props.onManageProviders}
        />
      </SettingsSection>
      <div className="settings-privacy-note">
        <ShieldCheck size={17} />
        <span>只有手动开始转写或开启自动转写后，录音才会发送到所选服务。</span>
      </div>
    </div>
  );
}
