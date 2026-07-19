import { useState } from "react";
import { ArrowLeft, Bot, SlidersHorizontal } from "lucide-react";
import type { AppSettings, SettingsBundle } from "../../../types/appSettings";
import type { ModelSettings, ProviderApiKeyUpdate } from "../../../types/modelSettings";
import { GeneralSettingsPanel } from "./GeneralSettingsPanel";
import { ModelSettingsPanel } from "./ModelSettingsPanel";
import "../styles/settings-page.css";

type SettingsCategory = "general" | "models";

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
          <span>{activeCategory === "general" ? "基础" : "模型服务"}</span>
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
        ) : (
          <ModelSettingsPanel
            settings={props.settings}
            initialError={props.initialError}
            onSave={props.onSave}
          />
        )}
      </div>
    </section>
  );
}
