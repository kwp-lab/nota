import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, Copy, FileText, LoaderCircle, Trash2 } from "lucide-react";
import type { AiTemplate, SaveAiTemplateRequest } from "../../../types";
import type { SettingsController } from "../hooks/useSettingsController";
import { InlineStatus } from "../SettingsPrimitives";
import { ProviderManagerShell, ResourceEmptyState } from "../providers/ProviderManagerShell";

const emptyDraft = (): SaveAiTemplateRequest => ({
  id: null,
  name: "自定义会议模板",
  description: "",
  taskInstructions: "",
  outputRequirements: "使用 Markdown 输出。",
  requiresSpeakerLabels: false,
});

const toDraft = (template: AiTemplate): SaveAiTemplateRequest => ({
  id: template.id,
  name: template.name,
  description: template.description,
  taskInstructions: template.taskInstructions,
  outputRequirements: template.outputRequirements,
  requiresSpeakerLabels: template.requiresSpeakerLabels,
});

export function AiTemplateManager(props: {
  controller: SettingsController;
  onBack: () => void;
  onDirtyChange: (dirty: boolean) => void;
}) {
  const [templates, setTemplates] = useState<AiTemplate[]>([]);
  const [selected, setSelected] = useState<AiTemplate | null>(null);
  const [draft, setDraft] = useState<SaveAiTemplateRequest | null>(null);
  const [baseline, setBaseline] = useState<SaveAiTemplateRequest | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const visibleTemplates = useMemo(
    () => templates.filter((template) => !template.archived),
    [templates],
  );
  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(baseline),
    [baseline, draft],
  );

  useEffect(() => props.onDirtyChange(dirty), [dirty, props.onDirtyChange]);
  useEffect(() => () => props.onDirtyChange(false), [props.onDirtyChange]);

  const refresh = useCallback(async (selectId?: string) => {
    setLoading(true);
    setError(null);
    try {
      const next = await props.controller.listAiTemplates();
      setTemplates(next);
      const nextSelected = next.find((template) => template.id === selectId)
        ?? next.find((template) => !template.archived)
        ?? null;
      setSelected(nextSelected);
      const nextDraft = nextSelected && !nextSelected.builtinKey ? toDraft(nextSelected) : null;
      setDraft(nextDraft);
      setBaseline(nextDraft);
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setLoading(false);
    }
  }, [props.controller]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const confirmDiscard = () => !dirty || confirm("AI 模板尚未保存。放弃这些更改吗？");
  const selectTemplate = (template: AiTemplate) => {
    if (!confirmDiscard()) return;
    setSelected(template);
    const nextDraft = template.builtinKey ? null : toDraft(template);
    setDraft(nextDraft);
    setBaseline(nextDraft);
    setError(null);
  };
  const addTemplate = () => {
    if (!confirmDiscard()) return;
    setSelected(null);
    setDraft(emptyDraft());
    setBaseline(null);
    setError(null);
  };

  const saveTemplate = async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const saved = await props.controller.saveAiTemplate(draft);
      await refresh(saved.id);
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setBusy(false);
    }
  };

  const cloneTemplate = async (template: AiTemplate) => {
    if (!confirmDiscard()) return;
    const name = prompt("输入复制后的模板名称", `${template.name}（自定义）`);
    if (!name?.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const cloned = await props.controller.cloneAiTemplate(template.id, name.trim());
      await refresh(cloned.id);
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setBusy(false);
    }
  };

  const archiveTemplate = async () => {
    if (!selected || selected.builtinKey || !confirm(`归档模板“${selected.name}”？已有 AI 文档仍可继续使用它。`)) return;
    setBusy(true);
    setError(null);
    try {
      await props.controller.archiveAiTemplate(selected.id);
      await refresh();
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setBusy(false);
    }
  };

  const list = visibleTemplates.map((template) => (
    <button
      type="button"
      key={template.id}
      className={template.id === selected?.id ? "selected" : ""}
      aria-label={`${template.name} ${template.builtinKey ? "内置" : "自定义"}`}
      onClick={() => selectTemplate(template)}
    >
      <span>
        <strong>{template.name}</strong>
        <small>{template.description || "暂无描述"}</small>
      </span>
      <em>{template.builtinKey ? "内置" : `r${template.revision}`}</em>
    </button>
  ));

  const detail = loading ? (
    <div className="settings-resource-empty"><LoaderCircle className="spinning" size={22} /><p>正在读取模板…</p></div>
  ) : error && !selected && !draft ? (
    <div className="settings-resource-empty" role="alert">
      <AlertTriangle size={22} />
      <p>模板读取失败：{error}</p>
      <button type="button" className="button secondary" onClick={() => void refresh()}>重试</button>
    </div>
  ) : draft ? (
    <div className="provider-detail-form template-detail-form">
      <header>
        <div><h2>{draft.id ? draft.name : "新建自定义模板"}</h2><span>自定义模板</span></div>
        {draft.id && (
          <button type="button" className="button danger-button" disabled={busy} onClick={() => void archiveTemplate()}>
            <Trash2 size={14} /> 归档
          </button>
        )}
      </header>
      <fieldset disabled={busy}>
        <legend>模板内容</legend>
        <div className="template-field-grid">
          <label><span>模板名称</span><input value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
          <label><span>用途说明</span><input value={draft.description} onChange={(event) => setDraft({ ...draft, description: event.target.value })} /></label>
          <label><span>任务指令</span><textarea rows={6} value={draft.taskInstructions} onChange={(event) => setDraft({ ...draft, taskInstructions: event.target.value })} /></label>
          <label><span>输出结构与要求</span><textarea rows={6} value={draft.outputRequirements} onChange={(event) => setDraft({ ...draft, outputRequirements: event.target.value })} /></label>
          <label className="template-checkbox">
            <span><strong>需要说话人标签</strong><small>无说话人分段的转写不能使用此模板。</small></span>
            <input
              type="checkbox"
              aria-label="需要带说话人标签的转写"
              checked={draft.requiresSpeakerLabels}
              onChange={(event) => setDraft({ ...draft, requiresSpeakerLabels: event.target.checked })}
            />
          </label>
        </div>
      </fieldset>
      {error && <InlineStatus tone="error"><AlertTriangle size={15} />{error}</InlineStatus>}
      <footer className="provider-detail-actions">
        <span>{dirty ? "有未保存的更改" : "模板已保存"}</span>
        <div>
          <button type="button" className="button secondary" disabled={!dirty || busy} onClick={() => setDraft(baseline)}>取消更改</button>
          <button type="button" className="button primary" disabled={!dirty || busy} onClick={() => void saveTemplate()}>
            {busy && <LoaderCircle size={14} className="spinning" />} 保存模板
          </button>
        </div>
      </footer>
    </div>
  ) : selected ? (
    <div className="provider-detail-form template-detail-form template-readonly">
      <header><div><h2>{selected.name}</h2><span>内置模板</span></div></header>
      <div className="template-readonly-body">
        <p>{selected.description || "暂无描述"}</p>
        <section><strong>任务指令</strong><pre>{selected.taskInstructions}</pre></section>
        <section><strong>输出结构与要求</strong><pre>{selected.outputRequirements}</pre></section>
      </div>
      {error && <InlineStatus tone="error"><AlertTriangle size={15} />{error}</InlineStatus>}
      <footer className="provider-detail-actions">
        <span>内置模板不能直接修改</span>
        <button type="button" className="button primary" disabled={busy} onClick={() => void cloneTemplate(selected)}>
          <Copy size={14} /> 复制为自定义模板
        </button>
      </footer>
    </div>
  ) : (
    <ResourceEmptyState
      title="尚无 AI 模板"
      description="创建模板后，可以控制 AI 文档的任务指令和输出结构。"
      actionLabel="新建模板"
      onAction={addTemplate}
    />
  );

  return (
    <ProviderManagerShell
      title="AI 文档 / 模板管理"
      description="查看内置模板，创建和维护自定义模板。"
      listTitle="AI 模板"
      count={visibleTemplates.length}
      onBack={() => confirmDiscard() && props.onBack()}
      onAdd={addTemplate}
      list={list}
      detail={detail}
    />
  );
}
