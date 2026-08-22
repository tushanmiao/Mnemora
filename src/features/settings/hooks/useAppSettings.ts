import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { AppSettings, SettingsBundle } from "../../../types/appSettings";
import { createInitialAppSettings } from "../../../types/appSettings";
import type { ModelSettings, ProviderApiKeyUpdate } from "../../../types/modelSettings";
import { createInitialModelSettings } from "../../../types/modelSettings";
import { loadApplicationSettings, saveApplicationSettings } from "../api/appSettings";
import { isTauriRuntime, loadModelSettings, persistModelSettings } from "../api/modelSettings";

export function useAppSettings() {
  const [appSettings, setAppSettings] = useState<AppSettings>(createInitialAppSettings);
  const [appSettingsError, setAppSettingsError] = useState<string | null>(null);
  const [modelSettings, setModelSettings] = useState<ModelSettings>(createInitialModelSettings);
  const [modelSettingsError, setModelSettingsError] = useState<string | null>(null);
  const [systemTheme, setSystemTheme] = useState<"light" | "dark">(() => (
    window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
  ));

  const resolvedTheme = appSettings.theme === "system" ? systemTheme : appSettings.theme;

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const updateSystemTheme = (event: MediaQueryListEvent) => {
      setSystemTheme(event.matches ? "dark" : "light");
    };
    media.addEventListener("change", updateSystemTheme);
    return () => media.removeEventListener("change", updateSystemTheme);
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let cancelled = false;

    void loadModelSettings()
      .then((settings) => {
        if (cancelled) return;
        setModelSettings(settings);
        setModelSettingsError(null);
      })
      .catch((error) => {
        if (!cancelled) setModelSettingsError(error instanceof Error ? error.message : String(error));
      });

    void loadApplicationSettings()
      .then((settings) => {
        if (cancelled) return;
        setAppSettings(settings);
        setAppSettingsError(null);
      })
      .catch((error) => {
        if (!cancelled) setAppSettingsError(error instanceof Error ? error.message : String(error));
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    let unlisten: (() => void) | undefined;
    void listen<AppSettings>("mnemora://app-settings-updated", (event) => {
      setAppSettings(event.payload);
      setAppSettingsError(null);
    }, { target: { kind: "WebviewWindow", label: "main" } }).then((dispose) => {
      unlisten = dispose;
    });
    return () => unlisten?.();
  }, []);

  const saveModelSettings = useCallback(async (
    nextSettings: ModelSettings,
    apiKeyUpdates: ProviderApiKeyUpdate[],
  ) => {
    if (!isTauriRuntime()) {
      const updateByProvider = new Map(
        apiKeyUpdates.map((update) => [update.providerId, update] as const),
      );
      const browserSettings = {
        ...nextSettings,
        providers: nextSettings.providers.map((provider) => {
          const update = updateByProvider.get(provider.id);
          return update ? { ...provider, hasApiKey: update.action === "set" } : provider;
        }),
      };
      setModelSettings(browserSettings);
      setModelSettingsError(null);
      return browserSettings;
    }

    const saved = await persistModelSettings(nextSettings, apiKeyUpdates);
    setModelSettings(saved);
    setModelSettingsError(null);
    return saved;
  }, []);

  const saveAppSettings = useCallback(async (nextSettings: AppSettings) => {
    if (!isTauriRuntime()) {
      setAppSettings(nextSettings);
      setAppSettingsError(null);
      return nextSettings;
    }

    const saved = await saveApplicationSettings(nextSettings);
    setAppSettings(saved);
    setAppSettingsError(null);
    return saved;
  }, []);

  const changeDefaultModel = useCallback(async (providerId: string, modelId: string) => {
    await saveModelSettings({
      ...modelSettings,
      defaultProviderId: providerId,
      defaultModelId: modelId,
    }, []);
  }, [modelSettings, saveModelSettings]);

  const changeNoteModel = useCallback(async (
    providerId: string | null,
    modelId: string | null,
  ) => {
    await saveModelSettings({
      ...modelSettings,
      noteProviderId: providerId,
      noteModelId: modelId,
    }, []);
  }, [modelSettings, saveModelSettings]);

  const applyImportedSettings = useCallback((bundle: SettingsBundle) => {
    setAppSettings(bundle.appSettings);
    setModelSettings(bundle.modelSettings);
    setAppSettingsError(null);
    setModelSettingsError(null);
  }, []);

  const toggleTheme = useCallback(() => {
    const nextSettings: AppSettings = {
      ...appSettings,
      theme: resolvedTheme === "light" ? "dark" : "light",
    };
    void saveAppSettings(nextSettings).catch((error) => {
      setAppSettingsError(error instanceof Error ? error.message : String(error));
    });
  }, [appSettings, resolvedTheme, saveAppSettings]);

  return {
    appSettings,
    appSettingsError,
    modelSettings,
    modelSettingsError,
    resolvedTheme,
    previewAppSettings: setAppSettings,
    saveAppSettings,
    saveModelSettings,
    changeDefaultModel,
    changeNoteModel,
    applyImportedSettings,
    toggleTheme,
  };
}
