import { useEffect, useState } from "react";
import { AlertCircle, CheckCircle2, MapPinOff, Save } from "lucide-react";
import type { AppSettings, PetSettings } from "../../../types/appSettings";
import { PetMascot } from "../../pet/PetMascot";
import "../styles/pet-settings.css";

type Feedback = { kind: "success" | "error"; message: string } | null;

export function PetSettingsPanel({
  settings,
  initialError,
  onSave,
}: {
  settings: AppSettings;
  initialError: string | null;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
}) {
  const [draft, setDraft] = useState(settings.pet);
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(
    initialError ? { kind: "error", message: initialError } : null,
  );

  useEffect(() => setDraft(settings.pet), [settings.pet]);

  const update = <Key extends keyof PetSettings>(key: Key, value: PetSettings[Key]) => {
    setDraft((current) => ({ ...current, [key]: value }));
    setFeedback(null);
  };

  const save = async () => {
    setSaving(true);
    setFeedback(null);
    try {
      const saved = await onSave({ ...settings, pet: draft });
      setDraft(saved.pet);
      setFeedback({ kind: "success", message: "桌面宠物设置已保存。" });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="settings-content pet-settings-content">
      <header className="settings-content-heading">
        <div>
          <h2>桌面宠物</h2>
          <span>低打扰地显示学习与 Agent 任务状态</span>
        </div>
        <button className="settings-button settings-button-primary" type="button" disabled={saving} onClick={() => void save()}>
          <Save size={15} />
          <span>{saving ? "保存中" : "保存设置"}</span>
        </button>
      </header>

      <div className="pet-settings-scroll">
        <section className="pet-preview-card">
          <div className="pet-preview-scene">
            <div className="pet-preview-bubble">
              <strong>记忆种子伙伴</strong>
              <span>只投影状态，不读取屏幕和麦克风</span>
            </div>
            <PetMascot state={draft.taskEvents ? "thinking" : "idle"} reducedMotion={draft.reducedMotion} />
          </div>
          <div className="pet-preview-copy">
            <span className="pet-preview-kicker">MIMO · MEMORY SEED</span>
            <h3>让后台任务有一个安静的表情</h3>
            <p>思考、读取来源、等待确认、完成和失败会映射为有限状态。宠物不会显示对话正文、附件路径或模型请求。</p>
          </div>
        </section>

        <section className="pet-settings-section">
          <h3>显示</h3>
          <PetRow label="启用桌面宠物" description="保存后创建独立透明窗口；关闭主窗口时一并销毁。">
            <Toggle checked={draft.enabled} onChange={(value) => update("enabled", value)} />
          </PetRow>
          <PetRow label="开机启动时显示" description="只有应用开机自启且宠物已启用时生效。">
            <Toggle checked={draft.showOnStartup} onChange={(value) => update("showOnStartup", value)} />
          </PetRow>
          <PetRow label="始终置顶">
            <Toggle checked={draft.alwaysOnTop} onChange={(value) => update("alwaysOnTop", value)} />
          </PetRow>
          <PetRow label="点击穿透" description="启用后窗口不接收鼠标；需要回到设置关闭。">
            <Toggle checked={draft.clickThrough} onChange={(value) => update("clickThrough", value)} />
          </PetRow>
          <PetRow label="显示状态气泡">
            <Toggle checked={draft.speechBubbles} onChange={(value) => update("speechBubbles", value)} />
          </PetRow>
        </section>

        <section className="pet-settings-section">
          <h3>外观与动态</h3>
          <PetRow label="宠物尺寸" description="窗口资源随尺寸调整，范围 120–280 px。">
            <Range value={draft.size} min={120} max={280} unit="px" onChange={(value) => update("size", value)} />
          </PetRow>
          <PetRow label="透明度">
            <Range value={draft.opacity} min={40} max={100} unit="%" onChange={(value) => update("opacity", value)} />
          </PetRow>
          <PetRow label="减少动态" description="停止呼吸、漂浮和庆祝动画，保留静态状态颜色。">
            <Toggle checked={draft.reducedMotion} onChange={(value) => update("reducedMotion", value)} />
          </PetRow>
          <PetRow label="跟随任务事件" description="只接收脱敏状态：思考、工具、等待、完成和失败。">
            <Toggle checked={draft.taskEvents} onChange={(value) => update("taskEvents", value)} />
          </PetRow>
          <div className="pet-position-reset">
            <button
              className="settings-button settings-button-secondary"
              type="button"
              onClick={() => setDraft((current) => ({ ...current, positionX: null, positionY: null }))}
            >
              <MapPinOff size={15} />
              <span>下次居中显示</span>
            </button>
          </div>
        </section>
      </div>

      {feedback ? (
        <div className={"settings-feedback settings-feedback-" + feedback.kind} role="status">
          {feedback.kind === "success" ? <CheckCircle2 size={17} /> : <AlertCircle size={17} />}
          <span>{feedback.message}</span>
        </div>
      ) : null}
    </section>
  );
}

function PetRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="pet-setting-row">
      <div>
        <strong>{label}</strong>
        {description ? <span>{description}</span> : null}
      </div>
      {children}
    </div>
  );
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <button
      className={"settings-toggle" + (checked ? " settings-toggle-active" : "")}
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  );
}

function Range({
  value,
  min,
  max,
  unit,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  unit: string;
  onChange: (value: number) => void;
}) {
  return (
    <div className="pet-range">
      <input type="range" min={min} max={max} value={value} onChange={(event) => onChange(Number(event.target.value))} />
      <output>{value}{unit}</output>
    </div>
  );
}
