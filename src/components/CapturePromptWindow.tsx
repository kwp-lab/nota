import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AlertTriangle, Radio, Square } from "lucide-react";
import { api } from "../api";
import type { CapturePrompt } from "../types";

const PROMPT_MIN_HEIGHT = 170;
const PROMPT_MAX_HEIGHT = 320;

export function CapturePromptWindow() {
  const promptElementRef = useRef<HTMLElement>(null);
  const layoutElementRef = useRef<HTMLDivElement>(null);
  const lastRequestedHeightRef = useRef(0);
  const [prompt, setPrompt] = useState<CapturePrompt | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    document.body.classList.add("capture-prompt-body");
    void api
      .onCapturePromptUpdated(setPrompt)
      .then((stopListening) => {
        if (disposed) stopListening();
        else unlisten = stopListening;
      })
      .catch(() => undefined);
    void api.getCapturePrompt().then(setPrompt).catch((reason) => {
      setError(String(reason));
    });
    return () => {
      disposed = true;
      unlisten?.();
      document.body.classList.remove("capture-prompt-body");
    };
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
      void api.resizeCapturePrompt(requestedHeight).catch(() => {
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
      await api.respondCapturePrompt(prompt.sessionId, stopAndSave);
    } catch (reason) {
      setError(String(reason));
      setSubmitting(false);
    }
  };

  return (
    <section
      ref={promptElementRef}
      className="capture-prompt"
      role="dialog"
      aria-labelledby="capture-prompt-title"
    >
      <div ref={layoutElementRef} className="capture-prompt-layout">
        <div className="capture-prompt-icon" aria-hidden="true">
          <AlertTriangle size={22} />
        </div>
        <div className="capture-prompt-content">
          <h1 id="capture-prompt-title">
            {prompt?.kind === "prolongedSilence"
              ? "应用已持续一段时间没有声音"
              : "应用音频捕获已中断"}
          </h1>
          {prompt ? (
            <p>
              {prompt.kind === "prolongedSilence"
                ? `Nota 已连续 3 分钟未检测到“${prompt.targetName}”的有效声音。会议可能仍在进行，你可以继续录音或停止并保存。`
                : `Nota 暂时无法继续获取“${prompt.targetName}”的声音。你可以继续等待恢复，或停止并保存录音。`}
            </p>
          ) : (
            <p>{error || "正在确认当前录音状态…"}</p>
          )}
          {error && prompt && (
            <div className="capture-prompt-error">{error}</div>
          )}
          <div className="capture-prompt-actions">
            <button
              className="button secondary"
              disabled={!prompt || submitting}
              onClick={() => void respond(false)}
            >
              <Radio size={16} />
              {prompt?.kind === "prolongedSilence" ? "继续录音" : "继续等待"}
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
