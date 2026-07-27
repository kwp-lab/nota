import { Folder, Info, Keyboard, Mic, ShieldCheck, X } from "lucide-react";
import type { AppSettings } from "../types";

interface SettingsDialogProps {
  open: boolean;
  firstRun: boolean;
  settings: AppSettings;
  microphoneCount: number;
  appVersion: string;
  onChange: (settings: AppSettings) => void;
  onChooseOutput: () => void;
  onOpenMicrophoneSettings: () => void;
  onCancel: () => void;
  onSave: () => void;
}

export function SettingsDialog(props: SettingsDialogProps) {
  if (!props.open) return null;
  return (
    <div className="modal-backdrop" role="presentation">
      <section className="modal settings-modal" role="dialog" aria-modal="true">
        {!props.firstRun && (
          <button className="icon-button modal-close" onClick={props.onCancel}>
            <X size={18} />
          </button>
        )}
        <div className="modal-icon">
          <ShieldCheck size={24} />
        </div>
        <h2>{props.firstRun ? "首次使用设置" : "录音设置"}</h2>
        <p className="muted">
          {props.firstRun
            ? "确认麦克风、保存目录和托盘行为。Nota 不需要账号，也不会联网上传。"
            : "这些设置只保存在本机。"}
        </p>

        <div className="settings-group">
          <div className="settings-heading">
            <Mic size={17} />
            <div>
              <strong>麦克风权限诊断</strong>
              <span>
                {props.microphoneCount > 0
                  ? `已发现 ${props.microphoneCount} 个可用输入设备`
                  : "未发现输入设备；请检查权限或连接"}
              </span>
            </div>
            <button className="text-button" onClick={props.onOpenMicrophoneSettings}>
              Windows 设置
            </button>
          </div>
          <label className="settings-row">
            <span>回声消除</span>
            <select
              value={props.settings.aecMode}
              onChange={(event) =>
                props.onChange({
                  ...props.settings,
                  aecMode: event.target.value as AppSettings["aecMode"],
                })
              }
            >
              <option value="auto">自动（扬声器开、耳机关）</option>
              <option value="on">强制开启</option>
              <option value="off">强制关闭</option>
            </select>
          </label>
        </div>

        <div className="settings-group">
          <div className="settings-heading">
            <Folder size={17} />
            <div>
              <strong>默认保存目录</strong>
              <span className="path-preview">{props.settings.outputDirectory}</span>
            </div>
            <button className="text-button" onClick={props.onChooseOutput}>更改</button>
          </div>
        </div>

        <div className="settings-group">
          <div className="settings-heading">
            <Keyboard size={17} />
            <div>
              <strong>全局快捷键</strong>
              <span>发生冲突时不会覆盖其他应用。</span>
            </div>
            <label className="switch-label">
              <input
                type="checkbox"
                checked={props.settings.shortcutsEnabled}
                onChange={(event) =>
                  props.onChange({
                    ...props.settings,
                    shortcutsEnabled: event.target.checked,
                  })
                }
              />
              启用
            </label>
          </div>
          {props.settings.shortcutsEnabled && (
            <div className="shortcut-grid">
              <label>
                <span>开始 / 暂停 / 继续</span>
                <input
                  value={props.settings.toggleShortcut}
                  onChange={(event) =>
                    props.onChange({
                      ...props.settings,
                      toggleShortcut: event.target.value,
                    })
                  }
                />
              </label>
              <label>
                <span>停止并保存</span>
                <input
                  value={props.settings.stopShortcut}
                  onChange={(event) =>
                    props.onChange({
                      ...props.settings,
                      stopShortcut: event.target.value,
                    })
                  }
                />
              </label>
            </div>
          )}
        </div>

        <div className="settings-group about-group">
          <div className="settings-heading">
            <Info size={17} />
            <div>
              <strong>关于此应用</strong>
              <span>Nota · 本地优先的 Windows 会议录音工具</span>
            </div>
            <span className="version-badge">v{props.appVersion}</span>
          </div>
          <div className="about-details">
            <span>Windows 11 x64</span>
            <span>本地处理 · 无账号 · 无遥测</span>
            <span>© 2026 Nota Contributors</span>
          </div>
        </div>

        {props.firstRun && (
          <div className="tray-note">
            关闭主窗口后应用会留在系统托盘。录音中退出时必须先选择“停止并保存”或取消退出。
          </div>
        )}

        <div className="modal-actions">
          {!props.firstRun && (
            <button className="button secondary" onClick={props.onCancel}>取消</button>
          )}
          <button className="button primary" onClick={props.onSave}>
            {props.firstRun ? "完成并进入应用" : "保存设置"}
          </button>
        </div>
      </section>
    </div>
  );
}
