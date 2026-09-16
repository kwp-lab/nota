import { Clipboard, FileDown, FileText, FolderOpen, Info, Link2, LoaderCircle, Plus, RefreshCw, WandSparkles } from "lucide-react";
import { AppTooltip } from "./AppTooltip";
import { DetailActionPopover } from "./DetailActionPopover";
import { DetailSelect } from "./DetailSelect";

interface ToolbarModel {
  documents: { id: string; title: string; templateName: string }[];
  versions: { id: string; label: string }[];
  documentId: string | null;
  versionId: string | null;
  canGenerate: boolean;
  generationHint: string;
  canRevise: boolean;
  canRead: boolean;
  canExport: boolean;
  exporting: boolean;
}

interface ToolbarActions {
  selectDocument: (id: string) => void;
  selectVersion: (id: string) => void;
  generate: () => void;
  revise: () => void;
  details: () => void;
  refresh: () => void;
  copy: () => void;
  exportPdf: () => void;
  copyPath: () => void;
  open: () => void;
  reveal: () => void;
}

export function AiDocumentToolbar({ model, actions }: { model: ToolbarModel; actions: ToolbarActions }) {
  return (
    <header className="ai-document-toolbar">
      <div className="ai-document-context">
        <div className="ai-document-selector">
          <DetailSelect
            aria-label="AI 文档"
            value={model.documentId ?? ""}
            disabled={!model.documents.length}
            onChange={(event) => actions.selectDocument(event.target.value)}
          >
            {!model.documents.length && <option value="">尚无文档</option>}
            {model.documents.map((document) => (
              <option key={document.id} value={document.id}>
                {document.title === document.templateName ? document.title : `${document.title} · ${document.templateName}`}
              </option>
            ))}
          </DetailSelect>
        </div>
        {model.documentId && (
          <div className="ai-version-selector">
            <DetailSelect
              aria-label="AI 文档版本"
              value={model.versionId ?? ""}
              disabled={!model.versions.length}
              onChange={(event) => actions.selectVersion(event.target.value)}
            >
              {model.versions.map((version) => (
                <option key={version.id} value={version.id}>{version.label}</option>
              ))}
            </DetailSelect>
          </div>
        )}
      </div>
      <div className="ai-document-actions">
        <AppTooltip content={model.generationHint} wrapDisabled={!model.canGenerate}>
          <button
            className="icon-button"
            aria-label="生成 AI 文档或新版本"
            disabled={!model.canGenerate}
            onClick={actions.generate}
          >
            <Plus size={16} />
          </button>
        </AppTooltip>
        {model.documentId && (
          <>
            <AppTooltip content="AI 修改：基于当前版本创建新版，原版本不会被覆盖" wrapDisabled={!model.canRevise}>
              <button className="icon-button" aria-label="AI 修改" disabled={!model.canRevise} onClick={actions.revise}>
                <WandSparkles size={16} />
              </button>
            </AppTooltip>
            {model.canRead && (
              <AppTooltip content="复制 Markdown">
                <button className="icon-button" aria-label="复制 Markdown" onClick={actions.copy}>
                  <Clipboard size={16} />
                </button>
              </AppTooltip>
            )}
            <AppTooltip content={model.exporting ? "正在导出 PDF…" : "导出 PDF"} wrapDisabled={!model.canExport}>
              <button className="icon-button" aria-label="导出 PDF" disabled={!model.canExport} onClick={actions.exportPdf}>
                {model.exporting ? <LoaderCircle className="spin" size={16} /> : <FileDown size={16} />}
              </button>
            </AppTooltip>
          </>
        )}
        {model.documentId && (
          <DetailActionPopover label="更多文档操作">
            <button onClick={actions.details}><Info size={14} />查看生成详情</button>
            {model.canRead && (
              <>
                <button onClick={actions.refresh}><RefreshCw size={14} />刷新预览</button>
                <button onClick={actions.copyPath}><Link2 size={14} />复制文件路径</button>
                <button onClick={actions.open}><FileText size={14} />使用默认应用打开</button>
                <button onClick={actions.reveal}><FolderOpen size={14} />在资源管理器中显示</button>
              </>
            )}
          </DetailActionPopover>
        )}
      </div>
    </header>
  );
}
