import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../../../api";
import { llmProviderReady } from "../../../llm";
import type {
  AiTemplate,
  AppSettings,
  AsrProviderProbeRequest,
  LlmProviderProbeRequest,
  SaveAiTemplateRequest,
  SaveAsrProviderRequest,
  SaveLlmProviderRequest,
} from "../../../types";
import type { SettingsActions, SettingsModel } from "../types";

interface SettingsMutation {
  apply: (current: AppSettings) => AppSettings;
  persist: (next: AppSettings) => Promise<AppSettings>;
  coalesceKey?: "settings";
  waiters: Array<{
    resolve: () => void;
    reject: (error: unknown) => void;
  }>;
}

export function useSettingsController(model: SettingsModel, actions: SettingsActions) {
  const confirmedRef = useRef(model.settings);
  const queueRef = useRef<SettingsMutation[]>([]);
  const drainingRef = useRef(false);
  const modelRef = useRef(model);
  const actionsRef = useRef(actions);
  const [savingPreferences, setSavingPreferences] = useState(false);
  const [preferenceError, setPreferenceError] = useState<string | null>(null);

  modelRef.current = model;
  actionsRef.current = actions;

  useEffect(() => {
    if (!drainingRef.current && queueRef.current.length === 0) {
      confirmedRef.current = model.settings;
    }
  }, [model.settings]);

  const projectedSettings = useCallback(() =>
    queueRef.current.reduce(
      (current, mutation) => mutation.apply(current),
      confirmedRef.current,
    ), []);

  const drain = useCallback(async () => {
    if (drainingRef.current) return;
    drainingRef.current = true;
    setSavingPreferences(true);
    setPreferenceError(null);

    while (queueRef.current.length > 0) {
      const mutation = queueRef.current[0];
      const next = mutation.apply(confirmedRef.current);
      try {
        confirmedRef.current = await mutation.persist(next);
        queueRef.current.shift();
        setPreferenceError(null);
        mutation.waiters.forEach(({ resolve }) => resolve());
      } catch (error) {
        queueRef.current.shift();
        mutation.waiters.forEach(({ reject }) => reject(error));
        const message = String(error);
        setPreferenceError(message);
        actionsRef.current.onError(error);
      }
      actionsRef.current.onSettingsChange(projectedSettings());
    }

    drainingRef.current = false;
    setSavingPreferences(false);
  }, [projectedSettings]);

  const enqueue = useCallback((
    apply: SettingsMutation["apply"],
    persist: SettingsMutation["persist"],
    coalesceKey?: SettingsMutation["coalesceKey"],
  ) => {
    const result = new Promise<void>((resolve, reject) => {
      const pendingStart = drainingRef.current ? 1 : 0;
      const pending = queueRef.current.at(-1);
      if (
        coalesceKey
        && pending
        && queueRef.current.length > pendingStart
        && pending.coalesceKey === coalesceKey
      ) {
        const previousApply = pending.apply;
        pending.apply = (current) => apply(previousApply(current));
        pending.persist = persist;
        pending.waiters.push({ resolve, reject });
      } else {
        queueRef.current.push({ apply, persist, coalesceKey, waiters: [{ resolve, reject }] });
      }
    });
    actionsRef.current.onSettingsChange(projectedSettings());
    void drain();
    return result;
  }, [drain, projectedSettings]);

  const savePatch = useCallback((patch: Partial<AppSettings>) =>
    enqueue(
      (current) => ({ ...current, ...patch }),
      async (next) => {
        await api.saveSettings(next);
        return next;
      },
      "settings",
    ), [enqueue]);

  const setActiveAsrProvider = useCallback((id: string | null) =>
    enqueue(
      (current) => ({
        ...current,
        activeAsrProviderId: id,
        autoTranscribe: id ? current.autoTranscribe : false,
      }),
      () => api.setActiveAsrProvider(id),
    ), [enqueue]);

  const setActiveLlmProvider = useCallback((id: string | null) =>
    enqueue(
      (current) => ({ ...current, activeLlmProviderId: id }),
      () => api.setActiveLlmProvider(id),
    ), [enqueue]);

  const chooseDirectory = useCallback(async (
    setting: "outputDirectory" | "aiDocumentsDirectory",
  ) => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected !== "string") return false;
    await savePatch({ [setting]: selected });
    return true;
  }, [savePatch]);

  const refreshAsrState = useCallback(async () => {
    const [settings, providers] = await Promise.all([
      api.getSettings(),
      api.listAsrProviders(),
    ]);
    confirmedRef.current = settings;
    actionsRef.current.onSettingsChange(settings);
    actionsRef.current.onAsrProvidersChange(providers);
    return { settings, providers };
  }, []);

  const refreshLlmState = useCallback(async () => {
    const [settings, providers] = await Promise.all([
      api.getSettings(),
      api.listLlmProviders(),
    ]);
    confirmedRef.current = settings;
    actionsRef.current.onSettingsChange(settings);
    actionsRef.current.onLlmProvidersChange(providers);
    return { settings, providers };
  }, []);

  const saveAsrProvider = useCallback(async (request: SaveAsrProviderRequest) => {
    const saved = await api.saveAsrProvider(request);
    let state = await refreshAsrState();
    if (!state.settings.activeAsrProviderId) {
      const settings = await api.setActiveAsrProvider(saved.id);
      confirmedRef.current = settings;
      actionsRef.current.onSettingsChange(settings);
      state = { ...state, settings };
    }
    return saved;
  }, [refreshAsrState]);

  const deleteAsrProvider = useCallback(async (id: string) => {
    await api.deleteAsrProvider(id);
    await refreshAsrState();
  }, [refreshAsrState]);

  const saveLlmProvider = useCallback(async (request: SaveLlmProviderRequest) => {
    const saved = await api.saveLlmProvider(request);
    let state = await refreshLlmState();
    if (!state.settings.activeLlmProviderId && llmProviderReady(saved)) {
      const settings = await api.setActiveLlmProvider(saved.id);
      confirmedRef.current = settings;
      actionsRef.current.onSettingsChange(settings);
      state = { ...state, settings };
    }
    return saved;
  }, [refreshLlmState]);

  const deleteLlmProvider = useCallback(async (id: string) => {
    await api.deleteLlmProvider(id);
    await refreshLlmState();
  }, [refreshLlmState]);

  const completeFirstRun = useCallback(async (skipped: boolean) => {
    await savePatch({ firstRunComplete: true });
    actionsRef.current.onToast(
      skipped ? "info" : "success",
      skipped ? "已跳过首次设置，可以随时从侧边栏返回。" : "首次设置已完成",
    );
    actionsRef.current.onFirstRunComplete();
  }, [savePatch]);

  return useMemo(() => ({
    savingPreferences,
    preferenceError,
    savePatch,
    setActiveAsrProvider,
    setActiveLlmProvider,
    chooseOutputDirectory: () => chooseDirectory("outputDirectory"),
    chooseAiDocumentsDirectory: () => chooseDirectory("aiDocumentsDirectory"),
    openMicrophoneSettings: () => api.openMicrophoneSettings(),
    openLogDirectory: () => api.openLogDirectory(),
    saveAsrProvider,
    deleteAsrProvider,
    testAsrProvider: (request: AsrProviderProbeRequest) => api.testAsrProvider(request),
    listAsrModels: (request: AsrProviderProbeRequest) => api.listAsrModels(request),
    saveLlmProvider,
    deleteLlmProvider,
    testLlmProvider: (request: LlmProviderProbeRequest) => api.testLlmProvider(request),
    listLlmModels: (request: LlmProviderProbeRequest) => api.listLlmModels(request),
    listAiTemplates: (): Promise<AiTemplate[]> => api.listAiTemplates(),
    saveAiTemplate: (request: SaveAiTemplateRequest) => api.saveAiTemplate(request),
    cloneAiTemplate: (id: string, name: string) => api.cloneAiTemplate(id, name),
    archiveAiTemplate: (id: string) => api.archiveAiTemplate(id),
    completeFirstRun,
  }), [
    chooseDirectory,
    completeFirstRun,
    deleteAsrProvider,
    deleteLlmProvider,
    preferenceError,
    saveAsrProvider,
    saveLlmProvider,
    savePatch,
    savingPreferences,
    setActiveAsrProvider,
    setActiveLlmProvider,
  ]);
}

export type SettingsController = ReturnType<typeof useSettingsController>;
