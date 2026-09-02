import { useCallback, useEffect, useState } from "react";
import {
  Bot,
  CheckCircle2,
  FileText,
  Info,
  Keyboard,
  LoaderCircle,
  Mic,
  Settings2,
  ShieldCheck,
  TriangleAlert,
} from "lucide-react";
import { useSettingsController } from "./hooks/useSettingsController";
import { AiDocumentsSettingsPage } from "./pages/AiDocumentsSettingsPage";
import { RecordingSettingsPage } from "./pages/RecordingSettingsPage";
import { SetupPage } from "./pages/SetupPage";
import { TranscriptionSettingsPage } from "./pages/TranscriptionSettingsPage";
import {
  AboutSettingsPage,
  DiagnosticsSettingsPage,
  ShortcutsSettingsPage,
} from "./pages/UtilitySettingsPages";
import { AsrProviderManager } from "./providers/AsrProviderManager";
import { LlmProviderManager } from "./providers/LlmProviderManager";
import { AiTemplateManager } from "./templates/AiTemplateManager";
import {
  parentSettingsRoute,
  type SettingsRoute,
  type SettingsWorkspaceProps,
} from "./types";

const navigation: Array<{
  route: SettingsRoute;
  label: string;
  icon: typeof Mic;
}> = [
  { route: "recording", label: "录音与保存", icon: Mic },
  { route: "transcription", label: "语音转写", icon: Settings2 },
  { route: "aiDocuments", label: "AI 文档", icon: Bot },
  { route: "shortcuts", label: "快捷键", icon: Keyboard },
  { route: "diagnostics", label: "诊断", icon: FileText },
  { route: "about", label: "关于", icon: Info },
];

export function SettingsWorkspace(props: SettingsWorkspaceProps) {
  const controller = useSettingsController(props.model, props.actions);
  const [editorDirty, setEditorDirty] = useState(false);
  const [asrReturnRoute, setAsrReturnRoute] = useState<"setup" | "transcription">("transcription");
  const activeRoute = parentSettingsRoute(props.route);

  useEffect(() => {
    props.actions.onEditorDirtyChange(editorDirty);
    return () => props.actions.onEditorDirtyChange(false);
  }, [editorDirty, props.actions]);

  const requestRoute = useCallback((route: SettingsRoute) => {
    if (route === props.route) return;
    if (editorDirty && !confirm("当前编辑内容尚未保存。放弃这些更改吗？")) return;
    setEditorDirty(false);
    props.onRouteChange(route);
  }, [editorDirty, props]);

  const page = (() => {
    switch (props.route) {
      case "setup":
        return (
          <SetupPage
            model={props.model}
            controller={controller}
            onManageProviders={() => {
              setAsrReturnRoute("setup");
              requestRoute("asrProviders");
            }}
          />
        );
      case "recording":
        return <RecordingSettingsPage model={props.model} controller={controller} />;
      case "transcription":
        return (
          <TranscriptionSettingsPage
            model={props.model}
            controller={controller}
            onManageProviders={() => {
              setAsrReturnRoute("transcription");
              requestRoute("asrProviders");
            }}
          />
        );
      case "asrProviders":
        return (
          <AsrProviderManager
            model={props.model}
            controller={controller}
            onBack={() => props.onRouteChange(asrReturnRoute)}
            onDirtyChange={setEditorDirty}
          />
        );
      case "aiDocuments":
        return (
          <AiDocumentsSettingsPage
            model={props.model}
            controller={controller}
            onManageProviders={() => requestRoute("llmProviders")}
            onManageTemplates={() => requestRoute("aiTemplates")}
          />
        );
      case "llmProviders":
        return (
          <LlmProviderManager
            model={props.model}
            controller={controller}
            onBack={() => props.onRouteChange("aiDocuments")}
            onDirtyChange={setEditorDirty}
          />
        );
      case "aiTemplates":
        return (
          <AiTemplateManager
            controller={controller}
            onBack={() => props.onRouteChange("aiDocuments")}
            onDirtyChange={setEditorDirty}
          />
        );
      case "shortcuts":
        return <ShortcutsSettingsPage model={props.model} controller={controller} />;
      case "diagnostics":
        return <DiagnosticsSettingsPage controller={controller} />;
      case "about":
        return <AboutSettingsPage appVersion={props.model.appVersion} />;
    }
  })();

  return (
    <section className="settings-workspace">
      <aside className="settings-navigation" aria-label="设置分类">
        <header>
          <span><Settings2 size={19} /></span>
          <div><strong>设置</strong><small>本机偏好</small></div>
        </header>
        <nav>
          {!props.model.settings.firstRunComplete && (
            <button
              type="button"
              className={props.route === "setup" ? "active" : ""}
              aria-current={props.route === "setup" ? "page" : undefined}
              onClick={() => requestRoute("setup")}
            >
              <ShieldCheck size={17} />
              <span>开始使用</span>
            </button>
          )}
          {navigation.map((item) => {
            const Icon = item.icon;
            const selected = activeRoute === item.route;
            return (
              <button
                type="button"
                key={item.route}
                className={selected ? "active" : ""}
                aria-current={selected ? "page" : undefined}
                onClick={() => requestRoute(item.route)}
              >
                <Icon size={17} />
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>
        <footer>
          {controller.savingPreferences ? (
            <><LoaderCircle size={14} className="spinning" />正在自动保存…</>
          ) : controller.preferenceError ? (
            <><TriangleAlert size={14} />保存失败</>
          ) : (
            <><CheckCircle2 size={14} />更改自动保存</>
          )}
        </footer>
      </aside>
      <div className="settings-detail-pane">
        {props.model.recordingActive && (
          <div className="settings-recording-note" role="status">
            <TriangleAlert size={16} />
            当前录音不会被设置操作中断；录音相关更改将在下一次录音时生效。
          </div>
        )}
        {controller.preferenceError && (
          <div className="settings-save-error" role="alert">
            <TriangleAlert size={16} />
            <span>设置未能保存，已恢复为上次成功保存的值：{controller.preferenceError}</span>
          </div>
        )}
        {page}
      </div>
    </section>
  );
}
