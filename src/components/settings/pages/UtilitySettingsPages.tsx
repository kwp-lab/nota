import { useEffect, useState } from "react";
import { FileText, Info, Keyboard } from "lucide-react";
import type { SettingsController } from "../hooks/useSettingsController";
import { InlineStatus, SettingsCard, SettingsPageHeader, SettingsSection } from "../SettingsPrimitives";
import type { SettingsModel } from "../types";

export function ShortcutsSettingsPage(props: {
  model: SettingsModel;
  controller: SettingsController;
}) {
  const [toggleShortcut, setToggleShortcut] = useState(props.model.settings.toggleShortcut);
  const [stopShortcut, setStopShortcut] = useState(props.model.settings.stopShortcut);
  const [draftDirty, setDraftDirty] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (draftDirty) return;
    setToggleShortcut(props.model.settings.toggleShortcut);
    setStopShortcut(props.model.settings.stopShortcut);
  }, [draftDirty, props.model.settings.stopShortcut, props.model.settings.toggleShortcut]);

  const commit = async (shortcutsEnabled = props.model.settings.shortcutsEnabled) => {
    const nextToggle = toggleShortcut.trim();
    const nextStop = stopShortcut.trim();
    if (
      shortcutsEnabled === props.model.settings.shortcutsEnabled
      && nextToggle === props.model.settings.toggleShortcut
      && nextStop === props.model.settings.stopShortcut
    ) {
      setDraftDirty(false);
      return;
    }
    setError(null);
    try {
      await props.controller.savePatch({
        shortcutsEnabled,
        toggleShortcut: nextToggle,
        stopShortcut: nextStop,
      });
      setToggleShortcut(nextToggle);
      setStopShortcut(nextStop);
      setDraftDirty(false);
    } catch (nextError) {
      setError(String(nextError));
      setDraftDirty(true);
    }
  };

  return (
    <div className="settings-preference-page">
      <SettingsPageHeader
        title="快捷键"
        description="在 Nota 位于后台时控制录音；发生冲突时不会覆盖其他应用。"
      />
      <SettingsSection title="全局快捷键">
        <SettingsCard
          icon={<Keyboard size={17} />}
          title="启用全局快捷键"
          description="关闭后不会注册任何系统级按键。"
        >
          <input
            type="checkbox"
            aria-label="启用全局快捷键"
            checked={props.model.settings.shortcutsEnabled}
            onChange={(event) => void commit(event.target.checked)}
          />
        </SettingsCard>
        <div className="settings-shortcut-fields">
          <label>
            <span>开始 / 暂停 / 继续</span>
            <input
              aria-label="开始暂停快捷键"
              disabled={!props.model.settings.shortcutsEnabled}
              value={toggleShortcut}
              onChange={(event) => {
                setToggleShortcut(event.target.value);
                setDraftDirty(true);
              }}
              onBlur={() => void commit()}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void commit();
                }
              }}
            />
          </label>
          <label>
            <span>停止并保存</span>
            <input
              aria-label="停止保存快捷键"
              disabled={!props.model.settings.shortcutsEnabled}
              value={stopShortcut}
              onChange={(event) => {
                setStopShortcut(event.target.value);
                setDraftDirty(true);
              }}
              onBlur={() => void commit()}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void commit();
                }
              }}
            />
          </label>
        </div>
      </SettingsSection>
      {error && <InlineStatus tone="error">{error}</InlineStatus>}
    </div>
  );
}

export function DiagnosticsSettingsPage(props: {
  controller: SettingsController;
}) {
  return (
    <div className="settings-preference-page">
      <SettingsPageHeader
        title="诊断"
        description="查看 Nota 仅保存在本机的技术日志。"
      />
      <SettingsSection title="技术日志">
        <SettingsCard
          icon={<FileText size={17} />}
          title="诊断与日志"
          description="日志自动轮转，不包含录音、转写正文或 AI 请求与响应内容。"
        >
          <button
            type="button"
            className="button secondary"
            onClick={() => void props.controller.openLogDirectory().catch(() => undefined)}
          >
            打开日志目录
          </button>
        </SettingsCard>
      </SettingsSection>
    </div>
  );
}

export function AboutSettingsPage(props: { appVersion: string }) {
  return (
    <div className="settings-preference-page">
      <SettingsPageHeader
        title="关于"
        description="Nota 是本地优先的 Windows 会议录音工具。"
      />
      <SettingsSection title="应用信息">
        <SettingsCard
          icon={<Info size={17} />}
          title="Nota"
          description="本地录音 · 无账号 · 无遥测"
        >
          <span className="version-badge">v{props.appVersion}</span>
        </SettingsCard>
        <div className="settings-about-details">
          <span>Windows 11 x64</span>
          <span>© 2026 Nota Contributors</span>
        </div>
      </SettingsSection>
    </div>
  );
}
