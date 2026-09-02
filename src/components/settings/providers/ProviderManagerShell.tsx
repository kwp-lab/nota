import type { ReactNode } from "react";
import { ArrowLeft, Plus } from "lucide-react";

export function ProviderManagerShell(props: {
  title: string;
  description: string;
  listTitle: string;
  count: number;
  onBack: () => void;
  onAdd: () => void;
  list: ReactNode;
  detail: ReactNode;
}) {
  return (
    <section className="settings-manager-page">
      <header className="settings-manager-header">
        <button type="button" className="settings-back-button" onClick={props.onBack}>
          <ArrowLeft size={17} /> 返回
        </button>
        <div>
          <h1>{props.title}</h1>
          <p>{props.description}</p>
        </div>
        <button type="button" className="button primary" onClick={props.onAdd}>
          <Plus size={15} /> 添加
        </button>
      </header>
      <div className="settings-manager-grid">
        <aside className="settings-resource-pane">
          <div className="settings-resource-heading">
            <strong>{props.listTitle}</strong>
            <span>{props.count}</span>
          </div>
          <div className="settings-resource-list">{props.list}</div>
        </aside>
        <div className="settings-resource-detail">{props.detail}</div>
      </div>
    </section>
  );
}

export function ResourceEmptyState(props: {
  title: string;
  description: string;
  actionLabel: string;
  onAction: () => void;
}) {
  return (
    <div className="settings-resource-empty">
      <strong>{props.title}</strong>
      <p>{props.description}</p>
      <button type="button" className="button primary" onClick={props.onAction}>
        <Plus size={15} /> {props.actionLabel}
      </button>
    </div>
  );
}
