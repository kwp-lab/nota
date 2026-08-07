import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AlertTriangle, Radio, Square } from "lucide-react";
import { api } from "../api";
import type { MeetingEndPrompt } from "../types";

const PROMPT_MIN_HEIGHT = 170;
const PROMPT_MAX_HEIGHT = 320;

export function MeetingEndPromptWindow() {
  const promptElementRef = useRef<HTMLElement>(null);
  const layoutElementRef = useRef<HTMLDivElement>(null);
  const lastRequestedHeightRef = useRef(0);
  const [prompt, setPrompt] = useState<MeetingEndPrompt | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    document.body.classList.add("meeting-end-prompt-body");
    void api.getMeetingEndPrompt().then(setPrompt).catch((reason) => {
      setError(String(reason));
    });
    return () => document.body.classList.remove("meeting-end-prompt-body");
  }, []);

  useLayoutEffect(() => {
    const promptElement = promptElementRef.current;
    const layoutElement = layoutElementRef.current;
    if (!promptElement || !layoutElement) return;

    const resizeToContent = () => {
      const styles = window.getComputedStyle(promptElement);
      const frameHeight = [
        styles.paddingTop,
        styles.paddingBottom,
        styles.borderTopWidth,
        styles.borderBottomWidth,
      ].reduce((total, value) => total + (Number.parseFloat(value) || 0), 0);
      const requestedHeight = Math.min(
        PROMPT_MAX_HEIGHT,
        Math.max(
          PROMPT_MIN_HEIGHT,
          Math.ceil(layoutElement.getBoundingClientRect().height + frameHeight),
        ),
      );
      if (requestedHeight === lastRequestedHeightRef.current) return;
      lastRequestedHeightRef.current = requestedHeight;
      void api.resizeMeetingEndPrompt(requestedHeight).catch(() => {
        lastRequestedHeightRef.current = 0;
      });
    };

    resizeToContent();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(resizeToContent);
    observer.observe(layoutElement);
    return () => observer.disconnect();
  }, []);

  const respond = async (stopAndSave: boolean) => {
    if (!prompt || submitting) return;
    setSubmitting(true);
    setError("");
    try {
      await api.respondMeetingEndPrompt(prompt.sessionId, stopAndSave);
    } catch (reason) {
      setError(String(reason));
      setSubmitting(false);
    }
  };

  return (
    <section
      ref={promptElementRef}
      className="meeting-end-prompt"
      role="dialog"
      aria-labelledby="meeting-end-title"
    >
      <div ref={layoutElementRef} className="meeting-end-layout">
        <div className="meeting-end-icon" aria-hidden="true">
          <AlertTriangle size={22} />
        </div>
        <div className="meeting-end-content">
          <h1 id="meeting-end-title">会议貌似已经结束</h1>
          {prompt ? (
            <p>“{prompt.targetName}”进程已经退出，需要停止录音并保存吗？</p>
          ) : (
            <p>{error || "正在确认当前录音状态…"}</p>
          )}
          {error && prompt && <div className="meeting-end-error">{error}</div>}
          <div className="meeting-end-actions">
            <button
              className="button secondary"
              disabled={!prompt || submitting}
              onClick={() => void respond(false)}
            >
              <Radio size={16} />继续录音
            </button>
            <button
              className="button primary"
              disabled={!prompt || submitting}
              onClick={() => void respond(true)}
            >
              <Square size={14} fill="currentColor" />停止并保存
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}
