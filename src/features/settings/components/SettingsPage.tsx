import { lazy, Suspense, useCallback, useState } from "react";
import { ArrowLeft, BarChart3, Bot, Brain, Bug, Info, SlidersHorizontal, Sparkles } from "lucide-react";
import { RootErrorBoundary } from "../../../bootstrap/RootErrorBoundary";
import type { AppSettings, SettingsBundle } from "../../../types/appSettings";
import type { ModelSettings, ProviderApiKeyUpdate } from "../../../types/modelSettings";
import type { useSkills } from "../../skills/hooks/useSkills";
import { AboutSettingsPanel } from "./AboutSettingsPanel";
import { GeneralSettingsPanel } from "./GeneralSettingsPanel";
import { ModelSettingsPanel } from "./ModelSettingsPanel";
import { RequestDebugSettingsPanel } from "./RequestDebugSettingsPanel";
import { UsageSettingsPanel } from "./UsageSettingsPanel";
import "../styles/settings-page.css";

const SkillSettingsPanel = lazy(() => import("./SkillSettingsPanel").then((module) => ({
  default: module.SkillSettingsPanel,
})));
const MemorySettingsPanel = lazy(() => import("./MemorySettingsPanel").then((module) => ({
  default: module.MemorySettingsPanel,
})));

export type SettingsCategory = "general" | "models" | "skills" | "memory" | "usage" | "debug" | "about";

const CATEGORY_LABELS: Record<SettingsCategory, string> = {
  general: "基础",
  models: "模型服务",
  skills: "技能",
  memory: "记忆",
  usage: "用量",
  debug: "请求调试",
  about: "关于",
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
};

export function SettingsPage(props: SettingsPageProps) {
  const activeCategory = props.activeCategory;
  const [memoryDirty, setMemoryDirty] = useState(false);

  const changeCategory = useCallback((category: SettingsCategory) => {
    if (
      category !== activeCategory
      && activeCategory === "memory"
      && memoryDirty
      && !window.confirm("记忆中有未保存修改。离开此页面将放弃这些修改，是否继续？")
    ) return;
    setMemoryDirty(false);
    props.onCategoryChange(category);
  }, [activeCategory, memoryDirty, props]);

  const goBack = useCallback(() => {
    if (
      activeCategory === "memory"
      && memoryDirty
      && !window.confirm("记忆中有未保存修改。返回聊天将放弃这些修改，是否继续？")
    ) return;
    setMemoryDirty(false);
    props.onBack();
  }, [activeCategory, memoryDirty, props]);

  const categories = [
    { id: "general", label: "基础", icon: SlidersHorizontal },
    { id: "models", label: "模型服务", icon: Bot },
    { id: "skills", label: "技能", icon: Sparkles },
    { id: "memory", label: "记忆", icon: Brain },
    { id: "usage", label: "用量", icon: BarChart3 },
    { id: "debug", label: "请求调试", icon: Bug },
    { id: "about", label: "关于", icon: Info },
  ] satisfies Array<{ id: SettingsCategory; label: string; icon: typeof Bot }>;

  return (
    <section className="settings-page" aria-label="设置">
      <header className="settings-header">
        <button className="icon-button" type="button" title="返回聊天" onClick={goBack}>
          <ArrowLeft size={19} />
        </button>
        <div>
          <h1>设置</h1>
          <span>{CATEGORY_LABELS[activeCategory]}</span>
        </div>
      </header>

      <div className="settings-layout">
        <nav className="settings-nav" aria-label="设置分类">
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

        <RootErrorBoundary key={activeCategory} title={`${CATEGORY_LABELS[activeCategory]}设置加载失败`}>
          <Suspense fallback={<div className="settings-panel-loading">正在加载{CATEGORY_LABELS[activeCategory]}设置...</div>}>
            {activeCategory === "general" ? (
              <GeneralSettingsPanel
                settings={props.appSettings}
                modelSettings={props.settings}
                initialError={props.appSettingsError}
                onPreview={props.onPreviewAppSettings}
                onSave={props.onSaveAppSettings}
                onImported={props.onSettingsImported}
                onDefaultModelChange={props.onDefaultModelChange}
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
            ) : activeCategory === "usage" ? (
              <UsageSettingsPanel />
            ) : activeCategory === "debug" ? (
              <RequestDebugSettingsPanel settings={props.appSettings} onSave={props.onSaveAppSettings} />
            ) : (
              <AboutSettingsPanel />
            )}
          </Suspense>
        </RootErrorBoundary>
      </div>
    </section>
  );
}
