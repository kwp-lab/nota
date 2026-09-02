import { Folder, Mic, Volume2 } from "lucide-react";
import type { AecMode } from "../../../types";
import type { SettingsController } from "../hooks/useSettingsController";
import { SelectControl, SettingsCard, SettingsPageHeader, SettingsSection } from "../SettingsPrimitives";
import type { SettingsModel } from "../types";

export function RecordingSettingsPage(props: {
  model: SettingsModel;
  controller: SettingsController;
}) {
  const saveAecMode = (aecMode: AecMode) => {
    void props.controller.savePatch({ aecMode }).catch(() => undefined);
  };

  return (
    <div className="settings-preference-page">
      <SettingsPageHeader
        title="录音与保存"
        description="选择输入行为和新录音的默认保存位置。"
      />
      <SettingsSection title="音频输入">
        <SettingsCard
          icon={<Mic size={17} />}
          title="麦克风"
          description={props.model.microphoneCount > 0
            ? `已发现 ${props.model.microphoneCount} 个可用输入设备`
            : "未发现输入设备，请检查权限或连接"}
        >
          <button
            type="button"
            className="button secondary"
            onClick={() => void props.controller.openMicrophoneSettings().catch(() => undefined)}
          >
            Windows 设置
          </button>
        </SettingsCard>
        <SettingsCard
          icon={<Volume2 size={17} />}
          title="回声消除"
          description="自动模式会在使用扬声器时开启，使用耳机时关闭。"
        >
          <SelectControl
            aria-label="回声消除"
            value={props.model.settings.aecMode}
            onChange={(event) => saveAecMode(event.target.value as AecMode)}
          >
            <option value="auto">自动</option>
            <option value="on">强制开启</option>
            <option value="off">强制关闭</option>
          </SelectControl>
        </SettingsCard>
      </SettingsSection>
      <SettingsSection title="文件">
        <SettingsCard
          icon={<Folder size={17} />}
          title="默认保存目录"
          description={props.model.settings.outputDirectory}
          className="settings-path-card"
        >
          <button
            type="button"
            className="button secondary"
            onClick={() => void props.controller.chooseOutputDirectory().catch(() => undefined)}
          >
            更改
          </button>
        </SettingsCard>
      </SettingsSection>
    </div>
  );
}
