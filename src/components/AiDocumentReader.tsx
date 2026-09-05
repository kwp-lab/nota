import { useLayoutEffect, useRef } from "react";
import { FileText, LoaderCircle } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { AiDocumentContent } from "../types";

/** Reading positions are local UI state, never persisted with meeting content. */
export function AiDocumentReader(props: {
  versionId: string | null;
  loading: boolean;
  content: AiDocumentContent | null;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const positions = useRef(new Map<string, number>());
  useLayoutEffect(() => {
    if (ref.current && props.versionId && !props.loading) {
      ref.current.scrollTop = positions.current.get(props.versionId) ?? 0;
    }
  }, [props.versionId, props.loading]);
  return (
    <div ref={ref} className="ai-markdown-body" role="region" aria-label="AI 文档正文" tabIndex={0}
      onScroll={(event) => {
        if (props.versionId && !props.loading) positions.current.set(props.versionId, event.currentTarget.scrollTop);
      }}>
      {props.loading ? (
        <div className="ai-documents-loading"><LoaderCircle className="spin" />读取 Markdown…</div>
      ) : props.content ? (
        <div className="ai-markdown-content">
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={{
            img: ({ alt }) => <span className="ai-remote-image">[图片未自动加载：{alt || "无标题"}]</span>,
          }}>{props.content.markdown}</ReactMarkdown>
        </div>
      ) : (
        <div className="ai-documents-empty"><FileText size={24} /><p>选择一个已完成版本查看内容。</p></div>
      )}
    </div>
  );
}
