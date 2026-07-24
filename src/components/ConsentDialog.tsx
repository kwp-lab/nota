import { ClipboardCopy, ShieldCheck, X } from "lucide-react";

interface ConsentDialogProps {
  open: boolean;
  description: string;
  outputDirectory: string;
  template: string;
  confirmed: boolean;
  onConfirmedChange: (confirmed: boolean) => void;
  onCopy: () => void;
  onCancel: () => void;
  onStart: () => void;
}

export function ConsentDialog(props: ConsentDialogProps) {
  if (!props.open) return null;
  return (
    <div className="modal-backdrop" role="presentation">
      <section className="modal" role="dialog" aria-modal="true">
        <button className="icon-button modal-close" onClick={props.onCancel}>
          <X size={18} />
        </button>
        <div className="modal-icon">
          <ShieldCheck size={24} />
        </div>
        <h2>首次录音提示</h2>
        <p className="muted">
          录音可能涉及隐私和当地法律要求。请在需要时告知参会者；确认后，后续开始录音将不再显示此提示。
        </p>
        <dl className="summary-list">
          <div>
            <dt>录音来源</dt>
            <dd>{props.description}</dd>
          </div>
          <div>
            <dt>保存位置</dt>
            <dd title={props.outputDirectory}>{props.outputDirectory}</dd>
          </div>
          <div>
            <dt>预计大小</dt>
            <dd>约 30 MB / 小时</dd>
          </div>
        </dl>
        <button className="copy-notice" onClick={props.onCopy}>
          <ClipboardCopy size={16} />
          复制参会者告知话术
        </button>
        <label className="consent-check">
          <input
            type="checkbox"
            checked={props.confirmed}
            onChange={(event) => props.onConfirmedChange(event.target.checked)}
          />
          <span>我已了解上述提示，后续开始录音时不再提醒</span>
        </label>
        <div className="modal-actions">
          <button className="button secondary" onClick={props.onCancel}>
            取消
          </button>
          <button
            className="button primary"
            disabled={!props.confirmed}
            onClick={props.onStart}
          >
            确认并开始录音
          </button>
        </div>
      </section>
    </div>
  );
}
