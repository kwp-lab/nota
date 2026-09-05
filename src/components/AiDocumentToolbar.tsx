import { Clipboard, FileText, FolderOpen, Info, Link2, Plus, RefreshCw, RotateCcw, WandSparkles } from "lucide-react";
import { AppTooltip } from "./AppTooltip";
import { DetailActionPopover } from "./DetailActionPopover";
import { DetailSelect } from "./DetailSelect";

interface ToolbarModel {
  documents: { id: string; title: string; templateName: string }[];
  versions: { id: string; label: string }[];
  documentId: string | null;
  versionId: string | null;
  canCreate: boolean;
  createHint: string;
  canRegenerate: boolean;
  canRevise: boolean;
  canRead: boolean;
}

interface ToolbarActions {
  selectDocument: (id: string) => void;
  selectVersion: (id: string) => void;
  create: () => void;
  regenerate: () => void;
  revise: () => void;
  details: () => void;
  refresh: () => void;
  copy: () => void;
  copyPath: () => void;
  open: () => void;
  reveal: () => void;
}

export function AiDocumentToolbar({ model, actions }: { model: ToolbarModel; actions: ToolbarActions }) {
  return (
    <header className="ai-document-toolbar">
      <DetailSelect aria-label="AI 文档" value={model.documentId ?? ""} disabled={!model.documents.length}
        onChange={(event) => actions.selectDocument(event.target.value)}>
        {!model.documents.length && <option value="">尚无文档</option>}
        {model.documents.map((document) => <option key={document.id} value={document.id}>
          {document.title === document.templateName ? document.title : `${document.title} · ${document.templateName}`}
        </option>)}
      </DetailSelect>
      <AppTooltip content={model.createHint} wrapDisabled={!model.canCreate}>
        <button className="icon-button" aria-label="生成新文档" disabled={!model.canCreate} onClick={actions.create}><Plus size={16} /></button>
      </AppTooltip>
      {model.documentId && <>
        <DetailSelect aria-label="AI 文档版本" value={model.versionId ?? ""} disabled={!model.versions.length}
          onChange={(event) => actions.selectVersion(event.target.value)}>
          {model.versions.map((version) => <option key={version.id} value={version.id}>{version.label}</option>)}
        </DetailSelect>
        <AppTooltip content="以当前所选版本为基础创建修改后的新版本；原版本不会被覆盖" wrapDisabled={!model.canRevise}>
          <button className="button secondary compact" disabled={!model.canRevise} onClick={actions.revise}><WandSparkles size={14} />AI修改</button>
        </AppTooltip>
        {model.canRead && <AppTooltip content="复制 Markdown"><button className="icon-button" aria-label="复制 Markdown" onClick={actions.copy}><Clipboard size={14} /></button></AppTooltip>}
        <AppTooltip content="生成详情"><button className="icon-button" aria-label="生成详情" onClick={actions.details}><Info size={16} /></button></AppTooltip>
        <DetailActionPopover label="更多文档操作">
          <AppTooltip content="不参考当前版本，使用当前转写创建全新版本；已有版本不会被覆盖" wrapDisabled={!model.canRegenerate}>
            <button className="button secondary compact" disabled={!model.canRegenerate} onClick={actions.regenerate}><RotateCcw size={14} />重新生成</button>
          </AppTooltip>
          {model.canRead && <>
            <button onClick={actions.refresh}><RefreshCw size={14} />刷新预览</button>
            <button onClick={actions.copyPath}><Link2 size={14} />复制文件路径</button>
            <button onClick={actions.open}><FileText size={14} />使用默认应用打开</button>
            <button onClick={actions.reveal}><FolderOpen size={14} />在资源管理器中显示</button>
          </>}
        </DetailActionPopover>
      </>}
    </header>
  );
}
