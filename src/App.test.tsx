import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import appIconUrl from "../src-tauri/icons/128x128.png";
import { api } from "./api";
import type {
  AudioImportBatchSnapshot,
  RecordingItem,
  RecordingSnapshot,
  TranscriptDocument,
} from "./types";

const dialogMocks = vi.hoisted(() => ({
  open: vi.fn(async () => null as string | string[] | null),
  save: vi.fn(async () => null as string | null),
}));

vi.mock("@tauri-apps/plugin-dialog", () => dialogMocks);

const testState = vi.hoisted(() => ({
  snapshot: {} as RecordingSnapshot,
  devicesError: false,
  devices: [] as Array<{
    id: string;
    name: string;
    direction: "render" | "capture";
    isDefaultCommunications: boolean;
    formFactor: string;
    active: boolean;
  }>,
  recoverable: [] as Array<Record<string, unknown>>,
  recordings: [] as RecordingItem[],
  transcript: null as TranscriptDocument | null,
  firstRunComplete: true,
  autoTranscribe: false,
  activeAsrProviderId: null as string | null,
  voiceprintProviderId: null as string | null,
  providers: [] as Array<{
    id: string;
    name: string;
    kind: "funAsr" | "openAiCompatible";
    baseUrl: string;
    modelId: string;
    hasApiKey: boolean;
    createdAt: string;
    updatedAt: string;
  }>,
  targets: [] as Array<{
    id: string;
    kind: "process";
    displayName: string;
    processId: number;
    executablePath: string;
    browser: boolean;
    priority: number;
  }>,
  requestStart: null as
    | ((mode?: "process" | "system" | "current") => void)
    | null,
  snapshotListener: null as
    | ((snapshot: RecordingSnapshot) => void)
    | null,
  audioImport: null as AudioImportBatchSnapshot | null,
  audioImportListener: null as
    | ((snapshot: AudioImportBatchSnapshot) => void)
    | null,
}));

vi.mock("./api", () => ({
  api: {
    getAppVersion: vi.fn(async () => "0.2.0"),
    listCaptureTargets: vi.fn(async () => testState.targets),
    listAudioDevices: vi.fn(async () => {
      if (testState.devicesError) throw new Error("麦克风权限已关闭");
      return testState.devices;
    }),
    getSettings: vi.fn(async () => ({
      outputDirectory: "C:\\Recordings",
      aecMode: "auto",
      microphoneEnabled: true,
      firstRunComplete: testState.firstRunComplete,
      shortcutsEnabled: true,
      toggleShortcut: "Ctrl+Alt+F9",
      stopShortcut: "Ctrl+Alt+F10",
      activeAsrProviderId: testState.activeAsrProviderId,
      voiceprintProviderId: testState.voiceprintProviderId,
      autoTranscribe: testState.autoTranscribe,
      aiDocumentsDirectory: "C:\\Users\\tester\\Documents\\Nota\\AI Documents",
      activeLlmProviderId: null,
    })),
    getSnapshot: vi.fn(async () => testState.snapshot),
    listRecordings: vi.fn(async () => testState.recordings),
    listRecoverable: vi.fn(async () => testState.recoverable),
    getAudioImportSnapshot: vi.fn(async () => testState.audioImport),
    startAudioImport: vi.fn(async (paths: string[]) => {
      testState.audioImport = {
        id: "import-batch",
        status: "running" as const,
        currentIndex: 0,
        total: paths.length,
        completed: 0,
        failed: 0,
        skipped: 0,
        items: paths.map((path, index) => ({
          id: `import-item-${index}`,
          fileName: path.split("\\").at(-1) ?? path,
          status: "queued" as const,
          progressCurrentMs: 0,
          progressTotalMs: 0,
          errorMessage: null,
          recordingId: null,
        })),
      };
      return testState.audioImport;
    }),
    cancelAudioImport: vi.fn(async () => {
      if (!testState.audioImport) throw new Error("没有导入任务");
      testState.audioImport = { ...testState.audioImport, status: "cancelled" };
      return testState.audioImport;
    }),
    prepareRecordingPlayback: vi.fn(async (id: string) => {
      const recording = testState.recordings.find((item) => item.id === id);
      if (!recording) throw new Error("录音不存在");
      return recording.path;
    }),
    deleteRecording: vi.fn(async (id: string) => {
      testState.recordings = testState.recordings.filter((item) => item.id !== id);
    }),
    copyTranscript: vi.fn(async () => undefined),
    exportTranscript: vi.fn(async () => undefined),
    revealTranscriptExport: vi.fn(async () => undefined),
    listAsrProviders: vi.fn(async () => testState.providers),
    listLlmProviders: vi.fn(async () => []),
    saveLlmProvider: vi.fn(async () => undefined),
    deleteLlmProvider: vi.fn(async () => undefined),
    testLlmProvider: vi.fn(async () => ({
      reachable: true,
      message: "服务可用",
      models: [],
    })),
    listLlmModels: vi.fn(async () => []),
    listAiTemplates: vi.fn(async () => []),
    saveAiTemplate: vi.fn(async () => undefined),
    cloneAiTemplate: vi.fn(async () => undefined),
    archiveAiTemplate: vi.fn(async () => undefined),
    getAiWorkspace: vi.fn(async () => ({
      profile: null,
      documents: [],
      templates: [],
      activeGeneration: null,
    })),
    previewAiGenerationRequest: vi.fn(async () => ({
      providerKind: "openAi",
      requestBody: {
        model: "test-model",
        instructions: "Follow the policy.",
        input: "Summarize the meeting.",
        max_output_tokens: 4_096,
        store: false,
      },
    })),
    copyAiRequestBody: vi.fn(),
    listAiDocumentVersions: vi.fn(async () => []),
    readAiDocumentVersion: vi.fn(async () => ({
      version: null,
      markdown: "",
      fileState: "missing" as const,
    })),
    readAiGenerationDetails: vi.fn(async (versionId: string) => ({
      versionId,
      requestBody: null,
      responseBody: null,
    })),
    copyAiGenerationJson: vi.fn(),
    generateAiDocument: vi.fn(async () => undefined),
    cancelAiGeneration: vi.fn(async () => undefined),
    updateAiMeetingContext: vi.fn(async () => undefined),
    relinkAiDocumentVersion: vi.fn(async () => undefined),
    findMovedAiDocumentVersion: vi.fn(async () => null),
    copyAiDocument: vi.fn(async () => undefined),
    openAiDocument: vi.fn(async () => undefined),
    revealAiDocument: vi.fn(async () => undefined),
    onAiStatus: vi.fn(async () => () => undefined),
    listParticipants: vi.fn(async () => []),
    onSnapshot: vi.fn(
      async (handler: (snapshot: RecordingSnapshot) => void) => {
        testState.snapshotListener = handler;
        return () => undefined;
      },
    ),
    onLevels: vi.fn(async () => () => undefined),
    onAsrStatus: vi.fn(async () => () => undefined),
    onAudioImportStatus: vi.fn(
      async (handler: (snapshot: AudioImportBatchSnapshot) => void) => {
        testState.audioImportListener = handler;
        return () => undefined;
      },
    ),
    saveSettings: vi.fn(async () => undefined),
    startRecording: vi.fn(async () => snapshot("recording")),
    pauseRecording: vi.fn(async () => snapshot("paused")),
    resumeRecording: vi.fn(async () => snapshot("recording")),
    stopRecording: vi.fn(async () => snapshot("completed")),
    setMicrophoneEnabled: vi.fn(async (microphone) => ({
      ...snapshot("recording"),
      microphone: {
        healthy: microphone !== null,
        label: "麦克风",
        detail: microphone === null ? "已关闭" : "已切换",
      },
      microphoneSelection: microphone,
      aecStatus: microphone === null ? "disabled" : "converging",
    })),
    onRequestStart: vi.fn(
      async (
        handler: (mode?: "process" | "system" | "current") => void,
      ) => {
      testState.requestStart = handler;
      return () => undefined;
      },
    ),
    onRequestExit: vi.fn(async () => () => undefined),
    saveAsrProvider: vi.fn(),
    deleteAsrProvider: vi.fn(async () => undefined),
    startTranscription: vi.fn(async () => ({
      status: "queued" as const,
      completedChunks: 0,
      totalChunks: 0,
      providerName: "FunASR",
      providerKind: "funAsr" as const,
      modelId: "paraformer",
      speakerCount: null,
      errorMessage: null,
      hasText: false,
      protocol: "nota_batch_v1" as const,
      voiceprintAnalysisSupported: true,
      progressPhase: "queued" as const,
      progressCurrent: 0,
      progressTotal: 0,
      progressUnit: "steps" as const,
    })),
    testAsrProvider: vi.fn(async () => ({
      reachable: true,
      level: "success",
      message: "服务可用",
      models: [],
      device: null,
    })),
    listAsrModels: vi.fn(async () => []),
    identifyRecordingSpeakers: vi.fn(),
    saveSpeakerIdentification: vi.fn(),
    updateRecordingSpeakerAssignments: vi.fn(),
    discardSpeakerIdentification: vi.fn(async () => undefined),
    renameParticipant: vi.fn(async () => []),
    deleteParticipant: vi.fn(async () => []),
    deleteVoiceprint: vi.fn(async () => []),
    getTranscript: vi.fn(async () => {
      if (!testState.transcript) throw new Error("没有转写结果");
      return testState.transcript;
    }),
    listTranscriptionVersions: vi.fn(async () => testState.transcript ? [{
      generation: testState.transcript.generation,
      providerName: testState.transcript.providerName,
      providerKind: testState.transcript.providerKind,
      modelId: testState.transcript.modelId,
      speakerCount: testState.transcript.speakerCount,
      protocol: testState.transcript.protocol,
      voiceprintAnalysisSupported: testState.transcript.voiceprintAnalysisSupported,
      createdAt: testState.transcript.updatedAt,
      completedAt: testState.transcript.completedAt ?? testState.transcript.updatedAt,
      isCurrent: true,
    }] : []),
    selectTranscriptionVersion: vi.fn(async () => {
      if (!testState.transcript) throw new Error("没有转写结果");
      return testState.transcript;
    }),
  },
}));

const snapshot = (state: RecordingSnapshot["state"]): RecordingSnapshot => ({
  sessionId: state === "idle" ? null : "session",
  state,
  startedAt: state === "idle" ? null : new Date().toISOString(),
  activeDurationMs: 12_000,
  bytesWritten: 120_000,
  outputPath: null,
  system: { healthy: true, label: "会议声音" },
  microphone: { healthy: true, label: "麦克风" },
  microphoneSelection:
    state === "idle" ? null : { kind: "followDefaultCommunications" },
  aecStatus: "enabled",
  fault: null,
});

describe("Nota UI states", () => {
  it("uses the packaged application icon for the sidebar brand", async () => {
    render(<App />);
    const logo = await screen.findByRole("img", { name: "Nota" });
    expect(logo).toHaveAttribute("src", appIconUrl);
    expect(logo.parentElement).toHaveClass("sidebar-brand");
    expect(logo.parentElement?.querySelector("svg")).toBeNull();
  });

  it("gives recordings the full-height shell and retains the active-recording return action", async () => {
    testState.snapshot = snapshot("recording");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "录音记录" }));
    await screen.findByText("录音仍在进行");
    expect(document.querySelector(".app-content")).toHaveClass("recordings-mode");
    expect(document.querySelector(".topbar")).toBeNull();
    expect(document.querySelector(".app-footer")).toBeNull();
    expect(document.querySelector(".app-sidebar")).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "返回录音控制" }));
    expect(document.querySelector(".topbar")).not.toBeNull();
    expect(document.querySelector(".app-footer")).not.toBeNull();
  });
  beforeEach(() => {
    testState.snapshot = snapshot("idle");
    testState.devicesError = false;
    testState.devices = [];
    testState.recoverable = [];
    testState.recordings = [];
    testState.transcript = null;
    testState.firstRunComplete = true;
    testState.autoTranscribe = false;
    testState.activeAsrProviderId = null;
    testState.voiceprintProviderId = null;
    testState.providers = [];
    testState.targets = [
      {
        id: "process:42",
        kind: "process",
        displayName: "Zoom",
        processId: 42,
        executablePath: "C:\\Zoom.exe",
        browser: false,
        priority: 100,
      },
    ];
    testState.requestStart = null;
    testState.snapshotListener = null;
    testState.audioImport = null;
    testState.audioImportListener = null;
    dialogMocks.open.mockResolvedValue(null);
    dialogMocks.save.mockResolvedValue(null);
    vi.clearAllMocks();
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  for (const state of ["idle", "recording", "paused", "interrupted"] as const) {
    it(`renders ${state}`, async () => {
      testState.snapshot = snapshot(state);
      render(<App />);
      expect(
        await screen.findByText(state === "idle" ? "准备好记录会议" : "会议录音进行中"),
      ).toBeInTheDocument();
      if (state !== "idle") {
        expect(await screen.findByText("停止并保存")).toBeInTheDocument();
      }
    });
  }

  it("shows a transient success toast after stopping and saving", async () => {
    testState.snapshot = snapshot("recording");
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "停止并保存" }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "录音已安全保存",
    );
    expect(vi.mocked(api.stopRecording)).toHaveBeenCalledOnce();
  });

  it("keeps automatic FunASR transcription on automatic speaker detection", async () => {
    testState.snapshot = snapshot("recording");
    testState.autoTranscribe = true;
    testState.activeAsrProviderId = "funasr";
    testState.providers = [{
      id: "funasr",
      name: "Local FunASR",
      kind: "funAsr",
      baseUrl: "http://127.0.0.1:8010/v1",
      modelId: "paraformer",
      hasApiKey: false,
      createdAt: "2026-08-04T00:00:00Z",
      updatedAt: "2026-08-04T00:00:00Z",
    }];

    render(<App />);

    await waitFor(() => expect(testState.snapshotListener).not.toBeNull());
    await waitFor(() => expect(api.listAsrProviders).toHaveBeenCalled());
    await act(async () => {
      testState.snapshotListener?.(snapshot("completed"));
    });

    await waitFor(() => expect(api.startTranscription).toHaveBeenCalledWith(
      "session",
      "funasr",
      null,
    ));
    expect(screen.queryByRole("dialog", { name: /转写/ })).not.toBeInTheDocument();
  });

  it("shows and deduplicates persistent recording faults received at runtime", async () => {
    render(<App />);
    await waitFor(() => expect(testState.snapshotListener).not.toBeNull());
    const faultySnapshot: RecordingSnapshot = {
      ...snapshot("recording"),
      fault: {
        component: "microphone",
        code: "DEVICE_DISCONNECTED",
        recoverable: true,
        userMessage: "麦克风连接已中断，正在重试。",
        occurredAt: "2026-07-30T12:00:00Z",
      },
    };

    act(() => testState.snapshotListener?.(faultySnapshot));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "麦克风连接已中断，正在重试。",
    );

    act(() => testState.snapshotListener?.(faultySnapshot));
    expect(
      screen.getAllByText("麦克风连接已中断，正在重试。"),
    ).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "关闭通知" }));
    await waitFor(() =>
      expect(
        screen.queryByText("麦克风连接已中断，正在重试。"),
      ).not.toBeInTheDocument(),
    );
    act(() => testState.snapshotListener?.(faultySnapshot));
    expect(
      screen.queryByText("麦克风连接已中断，正在重试。"),
    ).not.toBeInTheDocument();
  });

  it("shows microphone permission failures", async () => {
    testState.devicesError = true;
    render(<App />);
    expect(await screen.findByRole("alert")).toHaveTextContent("麦克风权限已关闭");
  });

  it("shows the current default device names in follow-default options", async () => {
    testState.devices = [
      {
        id: "render:default",
        name: "扬声器 (Realtek(R) Audio)",
        direction: "render",
        isDefaultCommunications: true,
        formFactor: "Speakers",
        active: true,
      },
      {
        id: "capture:default",
        name: "麦克风阵列 (Realtek(R) Audio)",
        direction: "capture",
        isDefaultCommunications: true,
        formFactor: "Microphone",
        active: true,
      },
    ];
    render(<App />);

    expect(
      await screen.findByRole("option", {
        name: "跟随默认通信设备（麦克风阵列 (Realtek(R) Audio)）",
      }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "全部系统声音" }));
    expect(
      screen.getByRole("option", {
        name: "跟随默认通信设备（扬声器 (Realtek(R) Audio)）",
      }),
    ).toBeInTheDocument();
  });

  it("switches microphones without stopping the active recording", async () => {
    testState.snapshot = snapshot("recording");
    testState.devices = [
      {
        id: "capture:built-in",
        name: "内置麦克风",
        direction: "capture",
        isDefaultCommunications: true,
        formFactor: "Microphone",
        active: true,
      },
      {
        id: "capture:usb",
        name: "USB 会议麦克风",
        direction: "capture",
        isDefaultCommunications: false,
        formFactor: "Microphone",
        active: true,
      },
    ];
    let finishSwitch: ((value: RecordingSnapshot) => void) | undefined;
    vi.mocked(api.setMicrophoneEnabled).mockImplementationOnce(
      () => new Promise((resolve) => {
        finishSwitch = resolve;
      }),
    );
    render(<App />);

    const selector = await screen.findByRole("combobox", {
      name: "本次录音麦克风",
    });
    fireEvent.change(selector, { target: { value: "capture:usb" } });

    expect(api.setMicrophoneEnabled).toHaveBeenCalledWith({
      kind: "fixed",
      endpointId: "capture:usb",
    });
    expect(selector).toBeDisabled();
    expect(screen.getByText("正在切换…")).toBeInTheDocument();

    await act(async () => {
      finishSwitch?.({
        ...snapshot("recording"),
        microphone: { healthy: true, label: "麦克风", detail: "已切换" },
        microphoneSelection: { kind: "fixed", endpointId: "capture:usb" },
        aecStatus: "converging",
      });
    });
    await waitFor(() => expect(selector).toHaveValue("capture:usb"));
    expect(selector).toBeEnabled();
    expect(await screen.findByRole("status")).toHaveTextContent(
      "麦克风已切换至 USB 会议麦克风",
    );
    expect(api.stopRecording).not.toHaveBeenCalled();
  });

  it("can disable only the current recording microphone", async () => {
    testState.snapshot = snapshot("recording");
    render(<App />);

    fireEvent.change(
      await screen.findByRole("combobox", { name: "本次录音麦克风" }),
      { target: { value: "off" } },
    );

    await waitFor(() => expect(api.setMicrophoneEnabled).toHaveBeenCalledWith(null));
    expect(await screen.findByText("本次录音已关闭")).toBeInTheDocument();
    expect(api.saveSettings).not.toHaveBeenCalled();
  });

  it("keeps the previous microphone selected when a live switch fails", async () => {
    testState.snapshot = snapshot("recording");
    testState.devices = [
      {
        id: "capture:usb",
        name: "USB 会议麦克风",
        direction: "capture",
        isDefaultCommunications: false,
        formFactor: "Microphone",
        active: true,
      },
    ];
    vi.mocked(api.setMicrophoneEnabled).mockRejectedValueOnce(
      new Error("无法启动所选麦克风"),
    );
    render(<App />);

    const selector = await screen.findByRole("combobox", {
      name: "本次录音麦克风",
    });
    fireEvent.change(selector, { target: { value: "capture:usb" } });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "无法启动所选麦克风",
    );
    await waitFor(() => expect(selector).toHaveValue("default"));
    expect(api.getSnapshot).toHaveBeenCalled();
    expect(api.stopRecording).not.toHaveBeenCalled();
  });

  it("shows the application version in settings", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.click(await screen.findByRole("button", { name: "关于" }));
    expect(await screen.findByRole("heading", { name: "关于" })).toBeInTheDocument();
    expect(screen.getByText("v0.2.0")).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "设置" })).not.toBeInTheDocument();
  });

  it("never exposes a saved ASR API key to the settings form", async () => {
    testState.activeAsrProviderId = "lan";
    testState.providers = [
      {
        id: "lan",
        name: "LAN FunASR",
        kind: "funAsr",
        baseUrl: "http://192.168.1.20:8000/v1",
        modelId: "sensevoice",
        hasApiKey: true,
        createdAt: "2026-07-28T00:00:00Z",
        updatedAt: "2026-07-28T00:00:00Z",
      },
    ];
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.click(await screen.findByRole("button", { name: "语音转写" }));
    fireEvent.click(await screen.findByRole("button", { name: /管理转写服务/ }));
    const key = screen.getByLabelText("API Key");
    expect(key).toHaveAttribute("type", "password");
    expect(key).toHaveValue("••••••••");
    expect(screen.queryByText(/明文保存在本机 Nota SQLite 数据库/)).not.toBeInTheDocument();
  });

  it("switches between the recorder and recording library from the sidebar", async () => {
    render(<App />);
    expect(await screen.findByText("准备好记录会议")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "录音记录" }));
    expect(await screen.findByText("还没有录音")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "录音" }));
    expect(await screen.findByText("准备好记录会议")).toBeInTheDocument();
  });

  it("selects supported phone recordings and starts a visible import batch", async () => {
    dialogMocks.open.mockResolvedValue([
      "C:\\Phone\\meeting.m4a",
      "C:\\Phone\\interview.mp3",
    ]);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "录音记录" }));
    fireEvent.click(screen.getAllByRole("button", { name: "导入录音" })[0]);

    await waitFor(() => expect(api.startAudioImport).toHaveBeenCalledWith([
      "C:\\Phone\\meeting.m4a",
      "C:\\Phone\\interview.mp3",
    ]));
    expect(dialogMocks.open).toHaveBeenCalledWith({
      directory: false,
      multiple: true,
      filters: [{
        name: "会议录音",
        extensions: ["mp3", "m4a", "wav", "flac"],
      }],
    });
    expect(await screen.findByText("准备导入 2 个文件")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "音频导入进度" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "录音" }));
    expect(screen.getByRole("button", { name: "开始录音" })).toBeDisabled();
  });

  it("disables audio import while a recording is active", async () => {
    testState.snapshot = snapshot("recording");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "录音记录" }));
    for (const button of screen.getAllByRole("button", { name: "导入录音" })) {
      expect(button).toBeDisabled();
    }
  });

  it("clears the details selection and confirms success after permanent deletion", async () => {
    testState.recordings = [
      {
        id: "first-recording",
        title: "待删除录音",
        path: "C:\\Recordings\\first.ogg",
        createdAt: "2026-07-31T01:00:00Z",
        durationMs: 60_000,
        sizeBytes: 1024,
        recovered: false,
        origin: "captured",
        sourceFileName: null,
        sourceFormat: null,
        importedAt: null,
        transcription: null,
      },
      {
        id: "second-recording",
        title: "下一条录音",
        path: "C:\\Recordings\\second.ogg",
        createdAt: "2026-07-31T02:00:00Z",
        durationMs: 60_000,
        sizeBytes: 1024,
        recovered: false,
        origin: "captured",
        sourceFileName: null,
        sourceFormat: null,
        importedAt: null,
        transcription: null,
      },
    ];
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "录音记录" }));
    expect(await screen.findByRole("heading", { name: "待删除录音" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "更多录音操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "永久删除" }));
    expect(await screen.findByRole("dialog", { name: "永久删除录音" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /同时删除关联的 AI Markdown 文件/ })).not.toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "永久删除" }));

    await waitFor(() =>
      expect(vi.mocked(api.deleteRecording)).toHaveBeenCalledWith("first-recording", true, false),
    );
    expect(await screen.findByRole("status")).toHaveTextContent("录音已永久删除");
    expect(screen.getByRole("heading", { name: "选择一条录音" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "下一条录音" })).not.toBeInTheDocument();
    expect(screen.getByText("下一条录音")).toBeInTheDocument();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("clarifies that deleting an imported recording preserves its source file", async () => {
    testState.recordings = [{
      id: "imported-recording",
      title: "手机会议",
      path: "C:\\Recordings\\phone-meeting.ogg",
      createdAt: "2026-08-11T01:00:00Z",
      durationMs: 60_000,
      sizeBytes: 1024,
      recovered: false,
      origin: "imported",
      sourceFileName: "phone-meeting.m4a",
      sourceFormat: "M4A",
      importedAt: "2026-08-11T02:00:00Z",
      transcription: null,
    }];
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "录音记录" }));
    expect(await screen.findByText("原文件：phone-meeting.m4a")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "更多录音操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "永久删除" }));

    expect(await screen.findByRole("dialog", { name: "永久删除录音" })).toHaveTextContent(
      "Nota 管理的音频副本将被永久删除，最初选择的文件不受影响。",
    );
  });

  it("offers an open-folder action after exporting a transcript", async () => {
    const transcription = {
      status: "completed" as const,
      completedChunks: 1,
      totalChunks: 1,
      providerName: "FunASR",
      providerKind: "funAsr" as const,
      modelId: "sensevoice",
      speakerCount: null,
      voiceprintAnalysisSupported: true,
      errorMessage: null,
      hasText: true,
      protocol: "nota_batch_v1" as const,
      progressPhase: null,
      progressCurrent: 1,
      progressTotal: 1,
      progressUnit: null,
    };
    testState.recordings = [
      {
        id: "export-recording",
        title: "项目例会",
        path: "C:\\Recordings\\meeting.ogg",
        createdAt: "2026-08-02T01:00:00Z",
        durationMs: 60_000,
        sizeBytes: 1024,
        recovered: false,
        origin: "captured",
        sourceFileName: null,
        sourceFormat: null,
        importedAt: null,
        transcription,
      },
    ];
    testState.transcript = {
      recordingId: "export-recording",
      generation: 1,
      status: "completed",
      providerName: "FunASR",
      providerKind: "funAsr",
      modelId: "sensevoice",
      speakerCount: null,
      protocol: "nota_batch_v1",
      voiceprintAnalysisSupported: true,
      language: "zh",
      text: "大家好。",
      segments: [
        {
          startMs: 0,
          endMs: 1_000,
          text: "大家好。",
          speaker: "speaker_0",
        },
      ],
      speakerNames: {},
      speakerAssignments: {},
      completedChunks: 1,
      totalChunks: 1,
      errorMessage: null,
      updatedAt: "2026-08-02T01:01:00Z",
      completedAt: "2026-08-02T01:01:00Z",
    };
    const exportPath = "C:\\Exports\\项目例会.txt";
    dialogMocks.save.mockResolvedValue(exportPath);

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "录音记录" }));
    fireEvent.click(await screen.findByRole("button", { name: "导出 TXT" }));

    await waitFor(() =>
      expect(vi.mocked(api.exportTranscript)).toHaveBeenCalledWith(
        "export-recording",
        exportPath,
      ),
    );
    expect(await screen.findByRole("status")).toHaveTextContent("转写文字已导出");
    fireEvent.click(screen.getByRole("button", { name: "打开文件夹" }));
    expect(vi.mocked(api.revealTranscriptExport)).toHaveBeenCalledWith(exportPath);
  });

  it("uses the full-screen settings workspace and auto-saves ordinary preferences", async () => {
    const confirm = vi.spyOn(window, "confirm");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    const aec = await screen.findByRole("combobox", { name: "回声消除" });
    fireEvent.change(aec, { target: { value: "off" } });
    await waitFor(() =>
      expect(vi.mocked(api.saveSettings)).toHaveBeenCalledWith(
        expect.objectContaining({ aecMode: "off", firstRunComplete: true }),
      ),
    );
    expect(screen.queryByText("保存设置")).not.toBeInTheDocument();
    expect(screen.queryByText("本地优先")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "录音" }));
    expect(confirm).not.toHaveBeenCalled();
    expect(await screen.findByText("准备好记录会议")).toBeInTheDocument();
  });

  it("opens first run as a non-blocking settings page and can skip it", async () => {
    testState.firstRunComplete = false;
    render(<App />);
    expect(await screen.findByRole("heading", { name: "开始使用 Nota" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "录音" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "稍后设置" }));
    await waitFor(() =>
      expect(vi.mocked(api.saveSettings)).toHaveBeenCalledWith(
        expect.objectContaining({ firstRunComplete: true }),
      ),
    );
    expect(await screen.findByText("准备好记录会议")).toBeInTheDocument();
  });

  it("keeps an active recording visible while browsing settings", async () => {
    testState.snapshot = snapshot("recording");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    expect(
      await screen.findByText(/当前录音不会被设置操作中断/),
    ).toBeInTheDocument();
  });

  it("offers crash recovery", async () => {
    testState.recoverable = [
      {
        id: "partial",
        title: "未完成录音",
        path: "partial.ogg",
        createdAt: new Date().toISOString(),
        durationMs: 0,
        sizeBytes: 1024,
        recovered: true,
      },
    ];
    render(<App />);
    const recovery = await screen.findByRole("button", {
      name: "发现 1 个未完成录音，前往录音记录恢复",
    });
    fireEvent.click(recovery);
    expect(await screen.findByText("1 个录音可恢复")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "恢复" })).toBeInTheDocument();
  });

  it("starts recording without a participant-notification prompt", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "开始录音" }));
    await waitFor(() => expect(vi.mocked(api.startRecording)).toHaveBeenCalled());
    const request = vi.mocked(api.startRecording).mock.calls.at(-1)?.[0];
    expect(request).not.toHaveProperty("consentConfirmed");
    expect(screen.queryByText("首次录音提示")).not.toBeInTheDocument();
  });

  it("starts directly from the tray without a participant-notification prompt", async () => {
    render(<App />);
    await waitFor(() => expect(testState.requestStart).not.toBeNull());
    act(() => testState.requestStart?.("current"));
    await waitFor(() => expect(vi.mocked(api.startRecording)).toHaveBeenCalled());
    expect(screen.queryByText("首次录音提示")).not.toBeInTheDocument();
  });

  it("starts system audio from the dedicated tray action", async () => {
    render(<App />);
    await waitFor(() => expect(testState.requestStart).not.toBeNull());
    act(() => testState.requestStart?.("system"));
    await waitFor(() =>
      expect(vi.mocked(api.startRecording)).toHaveBeenCalledWith(
        expect.objectContaining({
          capture: expect.objectContaining({ kind: "system" }),
        }),
      ),
    );
  });

  it("refreshes applications when the window regains focus", async () => {
    render(<App />);
    expect(
      await screen.findByRole("option", { name: "[Zoom.exe]: Zoom" }),
    ).toBeInTheDocument();
    testState.targets = [
      {
        id: "process:77",
        kind: "process",
        displayName: "腾讯会议",
        processId: 77,
        executablePath: "C:\\Program Files\\Tencent\\wemeetapp.exe",
        browser: false,
        priority: 90,
      },
      ...testState.targets,
    ];

    act(() => window.dispatchEvent(new Event("focus")));
    expect(
      await screen.findByRole("option", {
        name: "[wemeetapp.exe]: 腾讯会议",
      }),
    ).toBeInTheDocument();
  });

  it("rebinds the selected application after its process id changes", async () => {
    render(<App />);
    const source = await screen.findByRole("combobox", { name: "录音来源" });
    expect(source).toHaveValue("process:42");
    testState.targets = [
      {
        id: "process:84",
        kind: "process",
        displayName: "Zoom Meeting",
        processId: 84,
        executablePath: "c:\\zoom.exe",
        browser: false,
        priority: 100,
      },
    ];

    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(source).toHaveValue("process:84"));
  });

});
