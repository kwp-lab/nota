import { useId, useRef, useState } from "react";
import { EyeOff } from "lucide-react";
import { captureTargetExecutableName, normalizeCaptureExecutable } from "../../../captureTargets";
import type { SettingsController } from "../hooks/useSettingsController";
import type { SettingsModel } from "../types";
import { SettingsCard, SettingsSection } from "../SettingsPrimitives";

export function HiddenCaptureTargetsSettings(props: {
  model: SettingsModel;
  controller: SettingsController;
}) {
  const [executable, setExecutable] = useState("");
  const [error, setError] = useState<string | null>(null);
  const suggestionsId = useId();
  const formRef = useRef<HTMLFormElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const hidden = props.model.settings.hiddenCaptureExecutables;
  const suggestions = [...new Map(props.model.captureTargets.map((target) => [
    captureTargetExecutableName(target).toLowerCase(),
    target,
  ])).entries()].filter(([name]) => name && !hidden.includes(name));

  const add = async () => {
    try {
      const name = normalizeCaptureExecutable(executable);
      if (hidden.includes(name)) {
        setError("该进程已在隐藏名单中");
        return;
      }
      setError(null);
      await props.controller.setCaptureTargetHidden(name, true);
      setExecutable("");
      formRef.current?.querySelector("input")?.focus();
    } catch (failure) {
      setError(String(failure));
    }
  };

  const restore = async (name: string, index: number) => {
    try {
      setError(null);
      await props.controller.setCaptureTargetHidden(name, false);
      const buttons = listRef.current?.querySelectorAll<HTMLButtonElement>("button");
      const next = buttons?.[Math.min(index, buttons.length - 1)];
      (next ?? formRef.current?.querySelector<HTMLInputElement>("input"))?.focus();
    } catch (failure) {
      setError(String(failure));
    }
  };

  return (
    <div id="hidden-capture-executables">
      <SettingsSection title="已隐藏的录音来源">
        <p className="hidden-capture-help" id="hidden-capture-help">
          按进程文件名隐藏所有同名来源，重启 Nota 后仍然生效。
          仅影响“指定应用”候选列表；“全部系统声音”仍可能录到这些应用。
        </p>
        <form
          ref={formRef}
          className="hidden-capture-form"
          onSubmit={(event) => {
            event.preventDefault();
            void add();
          }}
        >
          <label htmlFor="hidden-capture-executable">进程文件名</label>
          <div>
            <input
              id="hidden-capture-executable"
              list={suggestionsId}
              value={executable}
              placeholder="例如 chrome.exe"
              autoComplete="off"
              aria-describedby={`hidden-capture-help hidden-capture-input-help${error ? " hidden-capture-error" : ""}`}
              aria-invalid={Boolean(error)}
              disabled={props.controller.savingPreferences}
              onChange={(event) => {
                setExecutable(event.target.value);
                setError(null);
              }}
            />
            <datalist id={suggestionsId}>
              {suggestions.map(([name, target]) => (
                <option value={name} key={name}>{target.displayName}</option>
              ))}
            </datalist>
            <button
              type="submit"
              className="button secondary"
              disabled={!executable.trim() || props.controller.savingPreferences}
            >添加到隐藏名单</button>
          </div>
          <p className="hidden-capture-help" id="hidden-capture-input-help">
            可输入完整的 .exe 文件名，或从建议中选择当前运行的应用。不区分大小写。
          </p>
        </form>
        {error && <p className="hidden-capture-error" id="hidden-capture-error" role="alert">{error}</p>}
        <div className="hidden-capture-list" ref={listRef}>
          {hidden.length === 0 ? (
            <p className="hidden-capture-help">尚未隐藏任何应用，也可以在录音来源列表中点击隐藏按钮。</p>
          ) : hidden.map((name, index) => (
            <SettingsCard key={name} icon={<EyeOff />} title={name}>
              <button
                type="button"
                className="button secondary compact"
                aria-label={`恢复显示 ${name}`}
                disabled={props.controller.savingPreferences}
                onClick={() => void restore(name, index)}
              >恢复显示</button>
            </SettingsCard>
          ))}
        </div>
      </SettingsSection>
    </div>
  );
}
