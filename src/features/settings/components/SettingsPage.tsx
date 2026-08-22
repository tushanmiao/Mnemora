import { lazy, Suspense, useCallback, useState } from "react";
import { ArrowLeft, BarChart3, Bot, Brain, Bug, Cloud, Database, Info, NotebookPen, PawPrint, SlidersHorizontal, Sparkles } from "lucide-react";
import { RootErrorBoundary } from "../../../bootstrap/RootErrorBoundary";
import type { AppSettings, SettingsBundle } from "../../../types/appSettings";
import type { ModelSettings, ProviderApiKeyUpdate } from "../../../types/modelSettings";
import type { useSkills } from "../../skills/hooks/useSkills";
import { useI18n } from "../../../i18n/I18nProvider";
import "../styles/settings-page.css";

const GeneralSettingsPanel = lazy(() => import("./GeneralSettingsPanel").then((module) => ({ default: module.GeneralSettingsPanel })));
const PetSettingsPanel = lazy(() => import("./PetSettingsPanel").then((module) => ({ default: module.PetSettingsPanel })));
const NoteSettingsPanel = lazy(() => import("./NoteSettingsPanel").then((module) => ({ default: module.NoteSettingsPanel })));
const ModelSettingsPanel = lazy(() => import("./ModelSettingsPanel").then((module) => ({ default: module.ModelSettingsPanel })));
const SkillSettingsPanel = lazy(() => import("./SkillSettingsPanel").then((module) => ({
  default: module.SkillSettingsPanel,
})));
const MemorySettingsPanel = lazy(() => import("./MemorySettingsPanel").then((module) => ({
  default: module.MemorySettingsPanel,
})));
const SyncSettingsPanel = lazy(() => import("./SyncSettingsPanel").then((module) => ({
  default: module.SyncSettingsPanel,
})));
const UsageSettingsPanel = lazy(() => import("./UsageSettingsPanel").then((module) => ({ default: module.UsageSettingsPanel })));
const StorageSettingsPanel = lazy(() => import("./StorageSettingsPanel").then((module) => ({ default: module.StorageSettingsPanel })));
const RequestDebugSettingsPanel = lazy(() => import("./RequestDebugSettingsPanel").then((module) => ({ default: module.RequestDebugSettingsPanel })));
const AboutSettingsPanel = lazy(() => import("./AboutSettingsPanel").then((module) => ({ default: module.AboutSettingsPanel })));

export type SettingsCategory = "general" | "pet" | "notes" | "models" | "skills" | "memory" | "storage" | "sync" | "usage" | "debug" | "about";

const CATEGORY_KEYS: Record<SettingsCategory, "settings.general" | "settings.pet" | "settings.notes" | "settings.models" | "settings.skills" | "settings.memory" | "settings.storage" | "settings.sync" | "settings.usage" | "settings.debug" | "settings.about"> = {
  general: "settings.general",
  pet: "settings.pet",
  notes: "settings.notes",
  models: "settings.models",
  skills: "settings.skills",
  memory: "settings.memory",
  storage: "settings.storage",
  sync: "settings.sync",
  usage: "settings.usage",
  debug: "settings.debug",
  about: "settings.about",
};

type SettingsPageProps = {
  settings: ModelSettings;
  appSettings: AppSettings;
  initialError: string | null;
  appSettingsError: string | null;
  activeCategory: SettingsCategory;
  skillState: ReturnType<typeof useSkills>;
  onBack: () => void;
  onCategoryChange: (category: SettingsCategory) => void;
  onSave: (
    settings: ModelSettings,
    apiKeyUpdates: ProviderApiKeyUpdate[],
  ) => Promise<ModelSettings>;
  onPreviewAppSettings: (settings: AppSettings) => void;
  onSaveAppSettings: (settings: AppSettings) => Promise<AppSettings>;
  onSettingsImported: (bundle: SettingsBundle) => void;
  onDefaultModelChange: (providerId: string, modelId: string) => Promise<void>;
  onNoteModelChange: (providerId: string | null, modelId: string | null) => Promise<void>;
};

export function SettingsPage(props: SettingsPageProps) {
  const { t } = useI18n();
  const activeCategory = props.activeCategory;
  const [memoryDirty, setMemoryDirty] = useState(false);

  const changeCategory = useCallback((category: SettingsCategory) => {
    if (
      category !== activeCategory
      && activeCategory === "memory"
      && memoryDirty
      && !window.confirm(t("settings.unsavedMemoryLeave"))
    ) return;
    setMemoryDirty(false);
    props.onCategoryChange(category);
  }, [activeCategory, memoryDirty, props]);

  const goBack = useCallback(() => {
    if (
      activeCategory === "memory"
      && memoryDirty
      && !window.confirm(t("settings.unsavedMemoryBack"))
    ) return;
    setMemoryDirty(false);
    props.onBack();
  }, [activeCategory, memoryDirty, props]);

  const categories = [
    { id: "general", label: t("settings.general"), icon: SlidersHorizontal },
    { id: "pet", label: t("settings.pet"), icon: PawPrint },
    { id: "notes", label: t("settings.notes"), icon: NotebookPen },
    { id: "models", label: t("settings.models"), icon: Bot },
    { id: "skills", label: t("settings.skills"), icon: Sparkles },
    { id: "memory", label: t("settings.memory"), icon: Brain },
    { id: "storage", label: t("settings.storage"), icon: Database },
    { id: "sync", label: t("settings.sync"), icon: Cloud },
    { id: "usage", label: t("settings.usage"), icon: BarChart3 },
    { id: "debug", label: t("settings.debug"), icon: Bug },
    { id: "about", label: t("settings.about"), icon: Info },
  ] satisfies Array<{ id: SettingsCategory; label: string; icon: typeof Bot }>;

  return (
    <section className="settings-page" aria-label={t("settings.title")}>
      <header className="settings-header">
        <button className="icon-button" type="button" title={t("settings.back")} aria-label={t("settings.back")} onClick={goBack}>
          <ArrowLeft size={19} />
        </button>
        <div>
          <h1>{t("settings.title")}</h1>
          <span>{t(CATEGORY_KEYS[activeCategory])}</span>
        </div>
      </header>

      <div className="settings-layout">
        <nav className="settings-nav" aria-label={t("settings.categories")}>
          {categories.map(({ id, label, icon: Icon }) => (
            <button
              className={`settings-nav-item${activeCategory === id ? " settings-nav-item-active" : ""}`}
              type="button"
              key={id}
              aria-current={activeCategory === id ? "page" : undefined}
              onClick={() => changeCategory(id)}
            >
              <Icon size={17} />
              <span>{label}</span>
            </button>
          ))}
        </nav>

        <RootErrorBoundary key={activeCategory} title={t("settings.loadFailed", { category: t(CATEGORY_KEYS[activeCategory]) })}>
          <Suspense fallback={<div className="settings-panel-loading">{t("settings.loading", { category: t(CATEGORY_KEYS[activeCategory]) })}</div>}>
            {activeCategory === "general" ? (
              <GeneralSettingsPanel
                settings={props.appSettings}
                modelSettings={props.settings}
                initialError={props.appSettingsError}
                onPreview={props.onPreviewAppSettings}
                onSave={props.onSaveAppSettings}
                onImported={props.onSettingsImported}
                onDefaultModelChange={props.onDefaultModelChange}
                onNoteModelChange={props.onNoteModelChange}
              />
            ) : activeCategory === "pet" ? (
              <PetSettingsPanel
                settings={props.appSettings}
                initialError={props.appSettingsError}
                onSave={props.onSaveAppSettings}
              />
            ) : activeCategory === "notes" ? (
              <NoteSettingsPanel
                settings={props.appSettings}
                initialError={props.appSettingsError}
                onPreview={props.onPreviewAppSettings}
                onSave={props.onSaveAppSettings}
              />
            ) : activeCategory === "models" ? (
              <ModelSettingsPanel settings={props.settings} initialError={props.initialError} onSave={props.onSave} />
            ) : activeCategory === "skills" ? (
              <SkillSettingsPanel state={props.skillState} />
            ) : activeCategory === "memory" ? (
              <MemorySettingsPanel
                settings={props.appSettings}
                onSaveSettings={props.onSaveAppSettings}
                onDirtyChange={setMemoryDirty}
              />
            ) : activeCategory === "storage" ? (
              <StorageSettingsPanel />
            ) : activeCategory === "sync" ? (
              <SyncSettingsPanel />
            ) : activeCategory === "usage" ? (
              <UsageSettingsPanel />
            ) : activeCategory === "debug" ? (
              <RequestDebugSettingsPanel settings={props.appSettings} onSave={props.onSaveAppSettings} />
            ) : (
              <AboutSettingsPanel
                settings={props.appSettings}
                onSaveSettings={props.onSaveAppSettings}
              />
            )}
          </Suspense>
        </RootErrorBoundary>
      </div>
    </section>
  );
}
