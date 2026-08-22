import { useEffect, useMemo, useState } from "react";
import { AlertCircle, Check, CheckCircle2, Download, FolderInput, FolderOpen, MapPinOff, RefreshCw, Save, Trash2 } from "lucide-react";
import type { AppSettings, PetSettings } from "../../../types/appSettings";
import { deletePet, importCodexPets, importPetPackage, installPetArchive, listPets, openPetDirectory } from "../../pet/api";
import { PetMascot } from "../../pet/PetMascot";
import { PetSprite } from "../../pet/PetSprite";
import type { PetDescriptor } from "../../pet/types";
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
  const [pets, setPets] = useState<PetDescriptor[]>([]);
  const [saving, setSaving] = useState(false);
  const [petBusy, setPetBusy] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(
    initialError ? { kind: "error", message: initialError } : null,
  );

  useEffect(() => setDraft(settings.pet), [settings.pet]);
  useEffect(() => { void refreshPets(); }, []);

  const selectedPet = useMemo(
    () => pets.find((pet) => pet.id === draft.selectedPetId) ?? pets[0] ?? null,
    [draft.selectedPetId, pets],
  );

  const update = <Key extends keyof PetSettings>(key: Key, value: PetSettings[Key]) => {
    setDraft((current) => ({ ...current, [key]: value }));
    setFeedback(null);
  };

  const refreshPets = async () => {
    try {
      const next = await listPets();
      setPets(next);
      if (next.length > 0 && !next.some((pet) => pet.id === draft.selectedPetId)) {
        setDraft((current) => ({ ...current, selectedPetId: "mimo" }));
      }
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    }
  };

  const choosePet = async (pet: PetDescriptor) => {
    const nextPet = { ...draft, selectedPetId: pet.id };
    setDraft(nextPet);
    setFeedback(null);
    try {
      const saved = await onSave({ ...settings, pet: nextPet });
      setDraft(saved.pet);
      setPets((current) => current.map((item) => ({ ...item, selected: item.id === saved.pet.selectedPetId })));
    } catch (error) {
      setDraft(settings.pet);
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    }
  };

  const save = async () => {
    setSaving(true);
    setFeedback(null);
    try {
      const saved = await onSave({ ...settings, pet: draft });
      setDraft(saved.pet);
      setFeedback({ kind: "success", message: "桌面宠物设置已保存。" });
      await refreshPets();
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setSaving(false);
    }
  };

  const importPet = async () => {
    setPetBusy(true);
    setFeedback(null);
    try {
      const next = await importPetPackage();
      if (!next) return;
      setPets(next);
      const selected = next.find((pet) => pet.selected);
      if (selected) setDraft((current) => ({ ...current, selectedPetId: selected.id }));
      setFeedback({ kind: "success", message: "宠物已安全导入并选中。" });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setPetBusy(false);
    }
  };

  const installPet = async () => {
    setPetBusy(true);
    setFeedback(null);
    try {
      const next = await installPetArchive();
      if (!next) return;
      setPets(next);
      const selected = next.find((pet) => pet.selected);
      if (selected) setDraft((current) => ({ ...current, selectedPetId: selected.id }));
      setFeedback({ kind: "success", message: "宠物 ZIP 已验证、安装并选中。" });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setPetBusy(false);
    }
  };

  const importFromCodex = async () => {
    setPetBusy(true);
    setFeedback(null);
    try {
      const result = await importCodexPets();
      if (!result) return;
      setPets(result.pets);
      if (result.selectedPetId) {
        setDraft((current) => ({ ...current, selectedPetId: result.selectedPetId ?? current.selectedPetId }));
      }
      const detail = result.failures.length > 0
        ? `；${result.failures.slice(0, 2).join("；")}${result.failures.length > 2 ? "；其余失败项已省略" : ""}`
        : "";
      setFeedback({
        kind: result.imported > 0 ? "success" : "error",
        message: `在 Codex 中发现 ${result.found} 个宠物，成功导入 ${result.imported} 个${detail}。`,
      });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setPetBusy(false);
    }
  };

  const removePet = async (pet: PetDescriptor) => {
    if (pet.source === "builtin" || !window.confirm(`删除本地宠物“${pet.displayName}”吗？`)) return;
    setPetBusy(true);
    setFeedback(null);
    try {
      const next = await deletePet(pet.id);
      setPets(next);
      const selected = next.find((item) => item.selected) ?? next[0];
      if (selected) setDraft((current) => ({ ...current, selectedPetId: selected.id }));
      setFeedback({ kind: "success", message: "本地宠物已删除。" });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setPetBusy(false);
    }
  };

  return (
    <section className="settings-content pet-settings-content">
      <header className="settings-content-heading">
        <div>
          <h2>桌面宠物</h2>
          <span>选择本地伙伴并低打扰地跟随 Chat 与深度笔记状态</span>
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
              <strong>{selectedPet?.displayName ?? "Mimo"}</strong>
              <span>{selectedPet?.description ?? "只投影状态，不读取屏幕和麦克风"}</span>
            </div>
            <div className="pet-preview-character">
              {selectedPet?.spritesheetUrl ? (
                <PetSprite pet={selectedPet} state={draft.taskEvents ? "thinking" : "idle"} reducedMotion={draft.reducedMotion} />
              ) : (
                <PetMascot state={draft.taskEvents ? "thinking" : "idle"} reducedMotion={draft.reducedMotion} />
              )}
            </div>
          </div>
          <div className="pet-preview-copy">
            <h3>本地宠物，不扩大 AI 权限</h3>
            <p>宠物包只包含 `pet.json` 和透明 WebP Sprite。可直接安装 hatch-pet 生成的 ZIP 或目录，不要求安装 Codex；导入时不会执行脚本或加载网络资源。</p>
          </div>
        </section>

        <section className="pet-settings-section pet-library-section">
          <div className="pet-library-heading">
            <div><h3>选择宠物</h3><span>直接安装 hatch-pet / Codex 兼容资源包</span></div>
            <div>
              <button className="settings-button settings-button-secondary" type="button" disabled={petBusy} onClick={() => void refreshPets()}><RefreshCw size={14} /><span>刷新</span></button>
              <button className="settings-button settings-button-secondary" type="button" disabled={petBusy} onClick={() => void openPetDirectory()}><FolderOpen size={14} /><span>打开目录</span></button>
              <button className="settings-button settings-button-primary" type="button" disabled={petBusy} onClick={() => void installPet()}><Download size={14} /><span>安装宠物包</span></button>
              <button className="settings-button settings-button-secondary" type="button" disabled={petBusy} onClick={() => void importPet()}><FolderInput size={14} /><span>导入目录</span></button>
              <button className="settings-button settings-button-secondary" type="button" disabled={petBusy} onClick={() => void importFromCodex()}><FolderInput size={14} /><span>从 Codex 迁移</span></button>
            </div>
          </div>
          <div className="pet-library-grid" role="radiogroup" aria-label="桌面宠物选择">
            {pets.map((pet) => (
              <article className={`pet-library-item${draft.selectedPetId === pet.id ? " is-selected" : ""}${!pet.compatible ? " is-incompatible" : ""}`} key={pet.id}>
                <button
                  className="pet-library-select"
                  type="button"
                  role="radio"
                  aria-checked={draft.selectedPetId === pet.id}
                  disabled={!pet.compatible}
                  onClick={() => void choosePet(pet)}
                >
                  <span className="pet-library-preview">
                    {pet.spritesheetUrl ? <PetSprite pet={pet} state="idle" reducedMotion /> : <PetMascot state="idle" reducedMotion />}
                  </span>
                  <span className="pet-library-copy"><strong>{pet.displayName}</strong><small>{pet.source === "builtin" ? "内置" : "本地"} · {pet.kind}</small></span>
                  {draft.selectedPetId === pet.id ? <Check size={15} /> : null}
                </button>
                {pet.compatibilityMessage ? <p>{pet.compatibilityMessage}</p> : null}
                {pet.source === "local" ? <button className="pet-library-delete" type="button" title={`删除 ${pet.displayName}`} disabled={petBusy} onClick={() => void removePet(pet)}><Trash2 size={14} /></button> : null}
              </article>
            ))}
          </div>
        </section>

        <section className="pet-settings-section">
          <h3>显示</h3>
          <PetRow label="启用桌面宠物" description="保存后创建独立透明窗口；拖动宠物主体即可移动，关闭主窗口时一并销毁。"><Toggle checked={draft.enabled} onChange={(value) => update("enabled", value)} /></PetRow>
          <PetRow label="开机启动时显示" description="只有应用开机自启且宠物已启用时生效。"><Toggle checked={draft.showOnStartup} onChange={(value) => update("showOnStartup", value)} /></PetRow>
          <PetRow label="始终置顶"><Toggle checked={draft.alwaysOnTop} onChange={(value) => update("alwaysOnTop", value)} /></PetRow>
          <PetRow label="点击穿透" description="启用后窗口不接收鼠标；需要回到设置关闭。"><Toggle checked={draft.clickThrough} onChange={(value) => update("clickThrough", value)} /></PetRow>
          <PetRow label="显示状态气泡"><Toggle checked={draft.speechBubbles} onChange={(value) => update("speechBubbles", value)} /></PetRow>
        </section>

        <section className="pet-settings-section">
          <h3>外观与动态</h3>
          <PetRow label="宠物尺寸" description="窗口资源随尺寸调整，范围 120–280 px。"><Range value={draft.size} min={120} max={280} unit="px" onChange={(value) => update("size", value)} /></PetRow>
          <PetRow label="透明度"><Range value={draft.opacity} min={40} max={100} unit="%" onChange={(value) => update("opacity", value)} /></PetRow>
          <PetRow label="减少动态" description="使用 Sprite 第一帧或静态状态颜色。"><Toggle checked={draft.reducedMotion} onChange={(value) => update("reducedMotion", value)} /></PetRow>
          <PetRow label="跟随任务事件" description="只接收脱敏状态：思考、工具、等待、完成和失败。"><Toggle checked={draft.taskEvents} onChange={(value) => update("taskEvents", value)} /></PetRow>
          <div className="pet-position-reset"><button className="settings-button settings-button-secondary" type="button" onClick={() => setDraft((current) => ({ ...current, positionX: null, positionY: null }))}><MapPinOff size={15} /><span>下次居中显示</span></button></div>
        </section>
      </div>

      {feedback ? <div className={`settings-feedback settings-feedback-${feedback.kind}`} role="status">{feedback.kind === "success" ? <CheckCircle2 size={17} /> : <AlertCircle size={17} />}<span>{feedback.message}</span></div> : null}
    </section>
  );
}

function PetRow({ label, description, children }: { label: string; description?: string; children: React.ReactNode }) {
  return <div className="pet-setting-row"><div><strong>{label}</strong>{description ? <span>{description}</span> : null}</div>{children}</div>;
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
  return <button className={`settings-toggle${checked ? " settings-toggle-active" : ""}`} type="button" role="switch" aria-checked={checked} onClick={() => onChange(!checked)}><span /></button>;
}

function Range({ value, min, max, unit, onChange }: { value: number; min: number; max: number; unit: string; onChange: (value: number) => void }) {
  return <div className="pet-range"><input type="range" min={min} max={max} value={value} onChange={(event) => onChange(Number(event.target.value))} /><output>{value}{unit}</output></div>;
}
