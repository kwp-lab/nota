import { convertFileSrc } from "@tauri-apps/api/core";
import {
  Fingerprint,
  LoaderCircle,
  Pencil,
  Play,
  Server,
  Trash2,
  UserRound,
} from "lucide-react";
import { useRef, useState } from "react";
import type { AsrProvider, ParticipantProfile, VoiceprintSample } from "../types";
import { AppTooltip } from "./AppTooltip";

interface VoiceprintsWorkspaceProps {
  participants: ParticipantProfile[];
  providers: AsrProvider[];
  providerId: string | null;
  loading: boolean;
  onProviderChange: (id: string | null) => void;
  onPreparePlayback: (recordingId: string) => Promise<string>;
  onRename: (id: string, displayName: string) => void;
  onDeleteParticipant: (id: string) => void;
  onDeleteSample: (id: string) => void;
  onError: (message: string) => void;
}

const formatDuration = (milliseconds: number) => {
  const seconds = Math.round(milliseconds / 1000);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${(seconds % 60).toString().padStart(2, "0")}`;
};

export function VoiceprintsWorkspace(props: VoiceprintsWorkspaceProps) {
  const funAsrProviders = props.providers.filter((provider) => provider.kind === "funAsr");
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [preview, setPreview] = useState<VoiceprintSample | null>(null);
  const [audioSource, setAudioSource] = useState("");

  const playSample = async (sample: VoiceprintSample) => {
    if (!sample.previewAvailable || !sample.sourceRecordingId) return;
    try {
      const path = await props.onPreparePlayback(sample.sourceRecordingId);
      setPreview(sample);
      setAudioSource(convertFileSrc(path));
      window.setTimeout(() => {
        if (!audioRef.current) return;
        audioRef.current.currentTime = sample.previewStartMs / 1_000;
        void audioRef.current.play();
      }, 0);
    } catch (error) {
      props.onError(String(error));
    }
  };

  return (
    <section className="voiceprints-workspace">
      <header className="voiceprints-header">
        <div>
          <p className="eyebrow">LOCAL VOICEPRINT LIBRARY</p>
          <h1>声纹管理</h1>
          <p>姓名和声纹只保存在本机。Nota ASR Server 仅提取匿名 CAM++ 向量。</p>
        </div>
        <Fingerprint size={34} />
      </header>

      <div className="voiceprint-provider-card">
        <Server size={20} />
        <div>
          <strong>声纹提取服务</strong>
          <span>这是独立配置，不会改变日常会议转写的默认服务。</span>
        </div>
        <select
          aria-label="声纹提取服务"
          value={props.providerId ?? ""}
          onChange={(event) => props.onProviderChange(event.target.value || null)}
        >
          <option value="">请选择 Nota ASR Server</option>
          {funAsrProviders.map((provider) => (
            <option value={provider.id} key={provider.id}>{provider.name}</option>
          ))}
        </select>
      </div>

      {props.loading ? (
        <div className="voiceprints-empty"><LoaderCircle className="spin" />读取本地声纹库…</div>
      ) : props.participants.length === 0 ? (
        <div className="voiceprints-empty">
          <Fingerprint size={34} />
          <h2>还没有保存的声纹</h2>
          <p>在已完成转写的录音详情中点击“说话人识别”，试听并确认姓名后会显示在这里。</p>
        </div>
      ) : (
        <div className="participant-grid">
          {props.participants.map((participant) => (
            <article className="participant-card" key={participant.id}>
              <header>
                <span className="participant-avatar"><UserRound size={20} /></span>
                <div>
                  <strong>{participant.displayName}</strong>
                  <small>{participant.samples.length} 个声纹样本</small>
                </div>
                <AppTooltip content="修改姓名">
                  <button
                    className="icon-button"
                    aria-label={`修改 ${participant.displayName} 的姓名`}
                    onClick={() => {
                      const name = prompt("输入新的参会人姓名", participant.displayName);
                      if (name?.trim() && name.trim() !== participant.displayName) {
                        props.onRename(participant.id, name.trim());
                      }
                    }}
                  >
                    <Pencil size={15} />
                  </button>
                </AppTooltip>
                <AppTooltip content="删除参会人">
                  <button
                    className="icon-button danger"
                    aria-label={`删除参会人 ${participant.displayName}`}
                    onClick={() => props.onDeleteParticipant(participant.id)}
                  >
                    <Trash2 size={15} />
                  </button>
                </AppTooltip>
              </header>
              <div className="voiceprint-samples">
                {participant.samples.map((sample) => (
                  <div className="voiceprint-sample" key={sample.id}>
                    <AppTooltip
                      content={sample.previewAvailable ? "试听原始录音片段" : "源录音已删除，无法试听"}
                      wrapDisabled={!sample.previewAvailable}
                    >
                      <button
                        className="sample-play"
                        aria-label={`试听 ${participant.displayName} 的原始录音片段`}
                        disabled={!sample.previewAvailable}
                        onClick={() => void playSample(sample)}
                      >
                        <Play size={13} fill="currentColor" />
                      </button>
                    </AppTooltip>
                    <div>
                      <strong>{sample.sourceRecordingTitle ?? "源录音已删除"}</strong>
                      <small>
                        {sample.sourceSpeaker} · {formatDuration(sample.previewStartMs)}
                      </small>
                    </div>
                    <AppTooltip content="删除声纹样本">
                      <button
                        className="sample-delete"
                        aria-label={`删除 ${participant.displayName} 的声纹样本`}
                        onClick={() => props.onDeleteSample(sample.id)}
                      >
                        <Trash2 size={13} />
                      </button>
                    </AppTooltip>
                  </div>
                ))}
              </div>
            </article>
          ))}
        </div>
      )}

      <audio
        ref={audioRef}
        src={audioSource || undefined}
        hidden={!preview}
        controls
        onTimeUpdate={() => {
          if (preview && audioRef.current
            && audioRef.current.currentTime * 1_000 >= preview.previewEndMs) {
            audioRef.current.pause();
          }
        }}
      />
    </section>
  );
}
