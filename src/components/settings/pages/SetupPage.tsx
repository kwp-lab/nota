import { CheckCircle2, Folder, Mic, Server, ShieldCheck } from "lucide-react";
import type { SettingsController } from "../hooks/useSettingsController";
import { SettingsCard, SettingsPageHeader, SettingsSection } from "../SettingsPrimitives";
import type { SettingsModel } from "../types";

export function SetupPage(props: {
  model: SettingsModel;
  controller: SettingsController;
  onManageProviders: () => void;
}) {
  const hasMicrophone = props.model.microphoneCount > 0;
  const hasProvider = props.model.asrProviders.length > 0;

  return (
    <div className="settings-preference-page settings-setup-page">
      <SettingsPageHeader
        title="开始使用 Nota"
        description="确认三个基础项目后即可开始录音；转写服务可以稍后配置。"
      />
      <div className="settings-setup-intro">
        <ShieldCheck size={22} />
        <div>
          <strong>录音始终可以离线使用</strong>
          <span>Nota 不要求账号，也不会在录音过程中自动上传音频。</span>
        </div>
      </div>

      <SettingsSection title="基础检查">
        <SettingsCard
          icon={<Mic size={17} />}
          title="麦克风"
          description={hasMicrophone
            ? `已发现 ${props.model.microphoneCount} 个可用输入设备`
            : "未发现输入设备，请检查 Windows 权限或设备连接"}
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
          icon={<Folder size={17} />}
          title="录音保存目录"
          description={props.model.settings.outputDirectory}
        >
          <button
            type="button"
            className="button secondary"
            onClick={() => void props.controller.chooseOutputDirectory().catch(() => undefined)}
          >
            更改
          </button>
        </SettingsCard>
        <SettingsCard
          icon={<Server size={17} />}
          title="语音转写服务（可选）"
          description={hasProvider
            ? `已配置 ${props.model.asrProviders.length} 个服务`
            : "不配置也可以正常录音，之后可随时添加"}
          onClick={props.onManageProviders}
          ariaLabel="管理语音转写服务"
        />
      </SettingsSection>

      <div className="settings-setup-actions">
        <span><CheckCircle2 size={15} />以上项目不会阻止你稍后继续调整设置</span>
        <div>
          <button
            type="button"
            className="button secondary"
            disabled={props.controller.savingPreferences}
            onClick={() => void props.controller.completeFirstRun(true).catch(() => undefined)}
          >
            稍后设置
          </button>
          <button
            type="button"
            className="button primary"
            disabled={props.controller.savingPreferences}
            onClick={() => void props.controller.completeFirstRun(false).catch(() => undefined)}
          >
            完成并开始
          </button>
        </div>
      </div>
    </div>
  );
}
