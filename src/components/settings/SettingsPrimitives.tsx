import type { ReactNode, SelectHTMLAttributes } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

export function SelectControl(props: SelectHTMLAttributes<HTMLSelectElement>) {
  const { children, ...selectProps } = props;
  return (
    <div className="select-wrap settings-select">
      <select {...selectProps}>{children}</select>
      <ChevronDown size={16} />
    </div>
  );
}

export function SettingsPageHeader(props: {
  title: string;
  description: string;
  action?: ReactNode;
  notice?: ReactNode;
}) {
  return (
    <header className="settings-detail-header">
      <div>
        <h1>{props.title}</h1>
        <p>{props.description}</p>
      </div>
      {props.action}
      {props.notice}
    </header>
  );
}

export function SettingsSection(props: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="settings-section">
      <h2>{props.title}</h2>
      <div className="settings-card-stack">{props.children}</div>
    </section>
  );
}

export function SettingsCard(props: {
  icon?: ReactNode;
  title: string;
  description?: ReactNode;
  children?: ReactNode;
  onClick?: () => void;
  ariaLabel?: string;
  className?: string;
}) {
  const content = (
    <>
      {props.icon && <span className="settings-card-icon">{props.icon}</span>}
      <span className="settings-card-copy">
        <strong>{props.title}</strong>
        {props.description && <small>{props.description}</small>}
      </span>
      <span className="settings-card-control">
        {props.children ?? (props.onClick ? <ChevronRight size={17} /> : null)}
      </span>
    </>
  );

  return props.onClick ? (
    <button
      type="button"
      className={`settings-card settings-card-link ${props.className ?? ""}`}
      aria-label={props.ariaLabel}
      onClick={props.onClick}
    >
      {content}
    </button>
  ) : (
    <div className={`settings-card ${props.className ?? ""}`}>{content}</div>
  );
}

export type ProviderFeedbackTone = "loading" | "success" | "warning" | "error";

export interface ProviderFeedback {
  tone: ProviderFeedbackTone;
  message: string;
}

export function InlineStatus(props: {
  tone?: "neutral" | "success" | "warning" | "error";
  children: ReactNode;
  role?: "status" | "alert";
}) {
  return (
    <div
      className={`settings-inline-status status-${props.tone ?? "neutral"}`}
      role={props.role ?? (props.tone === "error" ? "alert" : "status")}
    >
      {props.children}
    </div>
  );
}
