import {
  FileAudio,
  FolderOpen,
  LoaderCircle,
  MoreHorizontal,
  Pause,
  Play,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import type { RecordingItem } from "../types";

interface RecordingListProps {
  items: RecordingItem[];
  recoverable: RecordingItem[];
  onPreparePlayback: (id: string) => Promise<string>;
  onPlaybackError: (message: string) => void;
  onReveal: (id: string) => void;
  onDelete: (id: string) => void;
  onRecover: (id: string) => void;
  onDiscardRecovery: (id: string) => void;
  onRename: (id: string, currentTitle: string) => void;
  onPermanentDelete: (id: string) => void;
}

const formatDuration = (milliseconds: number) => {
  const seconds = Math.round(milliseconds / 1000);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  return hours > 0
    ? `${hours}:${minutes.toString().padStart(2, "0")}:${rest.toString().padStart(2, "0")}`
    : `${minutes}:${rest.toString().padStart(2, "0")}`;
};

const formatSize = (bytes: number) => {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
};

export function RecordingList({
  items,
  recoverable,
  onPreparePlayback,
  onPlaybackError,
  onReveal,
  onDelete,
  onRecover,
  onDiscardRecovery,
  onRename,
  onPermanentDelete,
}: RecordingListProps) {
  const audioRef = useRef<{ id: string; audio: HTMLAudioElement } | null>(null);
  const requestSequence = useRef(0);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const [loadingId, setLoadingId] = useState<string | null>(null);

  useEffect(
    () => () => {
      requestSequence.current += 1;
      const current = audioRef.current;
      if (current) {
        current.audio.pause();
        current.audio.src = "";
      }
      audioRef.current = null;
    },
    [],
  );

  const togglePlayback = async (item: RecordingItem) => {
    const current = audioRef.current;
    if (current?.id === item.id) {
      try {
        if (current.audio.paused) {
          await current.audio.play();
        } else {
          current.audio.pause();
        }
      } catch (error) {
        onPlaybackError(`无法播放“${item.title}”：${String(error)}`);
      }
      return;
    }

    requestSequence.current += 1;
    const sequence = requestSequence.current;
    if (current) {
      current.audio.pause();
      current.audio.src = "";
      audioRef.current = null;
    }
    setActiveId(null);
    setPlaying(false);
    setLoadingId(item.id);
    try {
      const path = await onPreparePlayback(item.id);
      if (requestSequence.current !== sequence) return;
      const audio = new Audio(convertFileSrc(path));
      audio.preload = "metadata";
      audio.onplay = () => {
        setActiveId(item.id);
        setPlaying(true);
        setLoadingId(null);
      };
      audio.onpause = () => setPlaying(false);
      audio.onended = () => {
        setActiveId(null);
        setPlaying(false);
        audioRef.current = null;
      };
      audio.onerror = () => {
        setActiveId(null);
        setPlaying(false);
        setLoadingId(null);
        audioRef.current = null;
        onPlaybackError(`无法播放“${item.title}”，文件可能已移动或格式不可用。`);
      };
      audioRef.current = { id: item.id, audio };
      setActiveId(item.id);
      await audio.play();
    } catch (error) {
      if (requestSequence.current === sequence) {
        setActiveId(null);
        setPlaying(false);
        setLoadingId(null);
        audioRef.current = null;
        onPlaybackError(`无法播放“${item.title}”：${String(error)}`);
      }
    }
  };

  return (
    <section className="recordings-section">
      <div className="section-heading">
        <div>
          <p className="eyebrow">LOCAL RECORDINGS</p>
          <h2>最近录音</h2>
        </div>
        <span className="count-pill">{items.length}</span>
      </div>
      {recoverable.length > 0 && (
        <div className="recovery-banner">
          <RotateCcw size={19} />
          <div>
            <strong>发现 {recoverable.length} 个未完成录音</strong>
            <p>可以恢复到最后一个完整音频页。</p>
          </div>
          <div className="recovery-actions">
            <button onClick={() => onRecover(recoverable[0].id)}>立即恢复</button>
            <button className="link-danger" onClick={() => onDiscardRecovery(recoverable[0].id)}>
              删除
            </button>
          </div>
        </div>
      )}
      <div className="recording-list">
        {items.length === 0 ? (
          <div className="empty-state">
            <FileAudio size={30} />
            <strong>还没有本地录音</strong>
            <span>完成的会议录音会出现在这里。</span>
          </div>
        ) : (
          items.map((item) => (
            <article className="recording-row" key={item.id}>
              <button
                className={`play-button ${activeId === item.id && playing ? "playing" : ""}`}
                aria-label={
                  activeId === item.id && playing
                    ? `暂停 ${item.title}`
                    : `播放 ${item.title}`
                }
                disabled={loadingId === item.id}
                onClick={() => void togglePlayback(item)}
              >
                {loadingId === item.id ? (
                  <LoaderCircle className="spin" size={17} />
                ) : activeId === item.id && playing ? (
                  <Pause size={17} fill="currentColor" />
                ) : (
                  <Play size={17} fill="currentColor" />
                )}
              </button>
              <div className="recording-meta">
                <strong>{item.title}</strong>
                <span>
                  {new Date(item.createdAt).toLocaleString("zh-CN", {
                    month: "numeric",
                    day: "numeric",
                    hour: "2-digit",
                    minute: "2-digit",
                  })}
                  · {formatDuration(item.durationMs)} · {formatSize(item.sizeBytes)}
                </span>
              </div>
              {item.recovered && <span className="recovered-pill">已恢复</span>}
              <button
                className="icon-button"
                title="打开所在文件夹"
                onClick={() => onReveal(item.id)}
              >
                <FolderOpen size={17} />
              </button>
              <button
                className="icon-button danger-hover"
                title="移入回收站"
                onClick={() => onDelete(item.id)}
              >
                <Trash2 size={17} />
              </button>
              <details className="row-menu">
                <summary className="icon-button" title="更多">
                  <MoreHorizontal size={17} />
                </summary>
                <div className="row-menu-popover">
                  <button onClick={() => onRename(item.id, item.title)}>重命名</button>
                  <button
                    className="danger"
                    onClick={() => onPermanentDelete(item.id)}
                  >
                    永久删除
                  </button>
                </div>
              </details>
            </article>
          ))
        )}
      </div>
    </section>
  );
}
