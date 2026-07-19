import { useState } from "react";
import { ArrowLeft, BarChart3, Bot, Bug, SlidersHorizontal } from "lucide-react";
import type { AppSettings, SettingsBundle } from "../../../types/appSettings";
import type { ModelSettings, ProviderApiKeyUpdate } from "../../../types/modelSettings";
import { GeneralSettingsPanel } from "./GeneralSettingsPanel";
import { ModelSettingsPanel } from "./ModelSettingsPanel";
import { RequestDebugSettingsPanel } from "./RequestDebugSettingsPanel";
import { UsageSettingsPanel } from "./UsageSettingsPanel";
import "../styles/settings-page.css";

type SettingsCategory = "general" | "models" | "usage" | "debug";

const CATEGORY_LABELS: Record<SettingsCategory, string> = {
  general: "基础",
  models: "模型服务",
  usage: "用量",
  debug: "请求调试",
};

type SettingsPageProps = {
  settings: ModelSettings;
  appSettings: AppSettings;
  initialError: string | null;
  appSettingsError: string | null;
  onBack: () => void;
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
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>("general");

  return (
    <section className="settings-page" aria-label="设置">
      <header className="settings-header">
        <button className="icon-button" type="button" title="返回聊天" onClick={props.onBack}>
          <ArrowLeft size={19} />
        </button>
        <div>
          <h1>设置</h1>
          <span>{CATEGORY_LABELS[activeCategory]}</span>
        </div>
      </header>

      <div className="settings-layout">
        <nav className="settings-nav" aria-label="设置分类">
          <button
            className={`settings-nav-item${activeCategory === "general" ? " settings-nav-item-active" : ""}`}
            type="button"
            aria-current={activeCategory === "general" ? "page" : undefined}
            onClick={() => setActiveCategory("general")}
          >
            <SlidersHorizontal size={17} />
            <span>基础</span>
          </button>
          <button
            className={`settings-nav-item${activeCategory === "models" ? " settings-nav-item-active" : ""}`}
            type="button"
            aria-current={activeCategory === "models" ? "page" : undefined}
            onClick={() => setActiveCategory("models")}
          >
            <Bot size={17} />
            <span>模型服务</span>
          </button>
          <button
            className={`settings-nav-item${activeCategory === "usage" ? " settings-nav-item-active" : ""}`}
            type="button"
            aria-current={activeCategory === "usage" ? "page" : undefined}
            onClick={() => setActiveCategory("usage")}
          >
            <BarChart3 size={17} />
            <span>用量</span>
          </button>
          <button
            className={`settings-nav-item${activeCategory === "debug" ? " settings-nav-item-active" : ""}`}
            type="button"
            aria-current={activeCategory === "debug" ? "page" : undefined}
            onClick={() => setActiveCategory("debug")}
          >
            <Bug size={17} />
            <span>请求调试</span>
          </button>
        </nav>

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
          <ModelSettingsPanel
            settings={props.settings}
            initialError={props.initialError}
            onSave={props.onSave}
          />
        ) : activeCategory === "usage" ? (
          <UsageSettingsPanel />
        ) : (
          <RequestDebugSettingsPanel
            settings={props.appSettings}
            onSave={props.onSaveAppSettings}
          />
        )}
      </div>
    </section>
  );
}
