import { useEffect, useMemo, useState } from "react";
import { AlertCircle, FolderOpen, LoaderCircle, RefreshCw, Save, ShieldCheck, Trash2 } from "lucide-react";
import type { AppSettings } from "../../../types/appSettings";
import {
  clearMemory,
  getMemoryDirectory,
  loadMemory,
  openMemoryDirectory,
  saveMemory,
  type MemoryLayer,
} from "../api/memory";
import "../styles/memory-settings.css";

const LIMITS: Record<MemoryLayer, number> = { l1: 5_000, l2: 1024 * 1024 };

type Props = {
  settings: AppSettings;
  onSaveSettings: (settings: AppSettings) => Promise<AppSettings>;
  onDirtyChange?: (dirty: boolean) => void;
};

export function MemorySettingsPanel({ settings, onSaveSettings, onDirtyChange }: Props) {
  const [activeLayer, setActiveLayer] = useState<MemoryLayer>("l1");
  const [contents, setContents] = useState<Partial<Record<MemoryLayer, string>>>({});
  const [savedContents, setSavedContents] = useState<Partial<Record<MemoryLayer, string>>>({});
  const [loadingLayer, setLoadingLayer] = useState<MemoryLayer | null>(null);
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [directory, setDirectory] = useState("");
  const content = contents[activeLayer] ?? "";
  const dirty = contents[activeLayer] !== savedContents[activeLayer];
  const bytes = useMemo(() => new TextEncoder().encode(content).length, [content]);
  const anyDirty = useMemo(() => Object.keys(contents).some((layer) => (
    contents[layer as MemoryLayer] !== savedContents[layer as MemoryLayer]
  )), [contents, savedContents]);

  useEffect(() => {
    let cancelled = false;
    void getMemoryDirectory()
      .then((path) => { if (!cancelled) setDirectory(path); })
      .catch(() => undefined);
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    onDirtyChange?.(anyDirty);
  }, [anyDirty, onDirtyChange]);

  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);

  useEffect(() => {
    if (contents[activeLayer] !== undefined) return;
    let cancelled = false;
    setLoadingLayer(activeLayer);
    setFeedback("");
    void loadMemory(activeLayer)
      .then((value) => {
        if (cancelled) return;
        setContents((current) => ({ ...current, [activeLayer]: value }));
        setSavedContents((current) => ({ ...current, [activeLayer]: value }));
      })
      .catch((error) => {
        if (!cancelled) setFeedback(errorMessage(error, "读取记忆失败。"));
      })
      .finally(() => {
        if (!cancelled) setLoadingLayer(null);
      });
    return () => { cancelled = true; };
  }, [activeLayer, contents]);

  useEffect(() => {
    if (!anyDirty) return;
    const preventClose = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", preventClose);
    return () => window.removeEventListener("beforeunload", preventClose);
  }, [anyDirty]);

  const changeLayer = (layer: MemoryLayer) => {
    if (layer === activeLayer) return;
    if (dirty) {
      if (!window.confirm("当前记忆尚未保存。切换层级将放弃这些修改，是否继续？")) return;
      setContents((current) => ({ ...current, [activeLayer]: savedContents[activeLayer] ?? "" }));
    }
    setActiveLayer(layer);
    setFeedback("");
  };

  const updateMemorySetting = async (key: keyof AppSettings["memory"], value: boolean) => {
    setSaving(true);
    setFeedback("");
    try {
      await onSaveSettings({
        ...settings,
        memory: { ...settings.memory, [key]: value },
      });
      setFeedback("记忆设置已保存。");
    } catch (error) {
      setFeedback(errorMessage(error, "保存记忆设置失败。"));
    } finally {
      setSaving(false);
    }
  };

  const persistActiveLayer = async () => {
    if (bytes > LIMITS[activeLayer]) return;
    setSaving(true);
    setFeedback("");
    try {
      await saveMemory(activeLayer, content);
      setSavedContents((current) => ({ ...current, [activeLayer]: content }));
      setFeedback(`${activeLayer.toUpperCase()} 已保存。`);
    } catch (error) {
      setFeedback(errorMessage(error, "保存记忆失败。"));
    } finally {
      setSaving(false);
    }
  };

  const reloadActiveLayer = async () => {
    if (dirty && !window.confirm("重新载入会放弃当前未保存修改，是否继续？")) return;
    setLoadingLayer(activeLayer);
    setFeedback("");
    try {
      const value = await loadMemory(activeLayer);
      setContents((current) => ({ ...current, [activeLayer]: value }));
      setSavedContents((current) => ({ ...current, [activeLayer]: value }));
      setFeedback(`${activeLayer.toUpperCase()} 已重新载入。`);
    } catch (error) {
      setFeedback(errorMessage(error, "重新读取记忆失败。"));
    } finally {
      setLoadingLayer(null);
    }
  };

  const clearActiveLayer = async () => {
    if (!window.confirm(`确定清空 ${activeLayer.toUpperCase()} 吗？此操作无法撤销。`)) return;
    setSaving(true);
    setFeedback("");
    try {
      await clearMemory(activeLayer);
      setContents((current) => ({ ...current, [activeLayer]: "" }));
      setSavedContents((current) => ({ ...current, [activeLayer]: "" }));
      setFeedback(`${activeLayer.toUpperCase()} 已清空。`);
    } catch (error) {
      setFeedback(errorMessage(error, "清空记忆失败。"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="settings-content memory-settings" aria-label="记忆设置">
      <div className="settings-content-heading">
        <div>
          <h2>记忆</h2>
          <span>L1 按需注入，L2 仅通过工具读取；默认不允许模型写入</span>
        </div>
        <button
          className="settings-button settings-button-secondary"
          type="button"
          title={directory || "打开记忆文件夹"}
          onClick={() => void openMemoryDirectory().then(setDirectory).catch((error) => {
            setFeedback(errorMessage(error, "打开记忆文件夹失败。"));
          })}
        >
          <FolderOpen size={15} />打开文件夹
        </button>
      </div>

      <code className="settings-code memory-directory" title={directory}>{directory || "正在读取记忆文件位置..."}</code>

      <div className="settings-section">
        <div className="settings-section-head">
          <ShieldCheck size={16} />
          <h3>权限</h3>
          <p>写入默认关闭；读取也受当前会话的 AI 权限约束</p>
        </div>
        <MemoryToggle label="启用记忆" description="允许当前应用使用 L1/L2 记忆能力" checked={settings.memory.enabled} disabled={saving} onChange={(value) => void updateMemorySetting("enabled", value)} />
        <MemoryToggle label="注入 L1" description="每次模型请求只注入最多 5,000 bytes 的在线记忆" checked={settings.memory.injectL1} disabled={saving || !settings.memory.enabled} onChange={(value) => void updateMemorySetting("injectL1", value)} />
        <MemoryToggle label="允许模型读取" description="允许 Agent 按权限读取或搜索记忆" checked={settings.memory.allowModelRead} disabled={saving || !settings.memory.enabled} onChange={(value) => void updateMemorySetting("allowModelRead", value)} />
        <MemoryToggle label="允许模型写入" description="敏感操作；仍受当前会话 AI 权限控制" checked={settings.memory.allowModelWrite} disabled={saving || !settings.memory.enabled} onChange={(value) => void updateMemorySetting("allowModelWrite", value)} />
      </div>

      <div className="memory-editor-shell">
        <div className="memory-tabs" role="tablist" aria-label="记忆层级">
          <button type="button" role="tab" aria-selected={activeLayer === "l1"} className={activeLayer === "l1" ? "memory-tab-active" : ""} onClick={() => changeLayer("l1")}>L1 在线记忆</button>
          <button type="button" role="tab" aria-selected={activeLayer === "l2"} className={activeLayer === "l2" ? "memory-tab-active" : ""} onClick={() => changeLayer("l2")}>L2 长期记忆</button>
          <span>{bytes.toLocaleString()} / {LIMITS[activeLayer].toLocaleString()} bytes</span>
        </div>
        {loadingLayer === activeLayer ? (
          <div className="settings-loading"><LoaderCircle className="settings-spin" size={18} />正在读取</div>
        ) : (
          <textarea
            value={content}
            aria-label={`${activeLayer.toUpperCase()} 记忆正文`}
            placeholder={activeLayer === "l1" ? "记录稳定且短小的偏好、背景和约束" : "记录需要按需搜索的长期事实"}
            onChange={(event) => setContents((current) => ({ ...current, [activeLayer]: event.target.value }))}
          />
        )}
        <div className="memory-editor-actions">
          {feedback ? <span className={feedback.includes("失败") ? "memory-feedback-error" : ""}><AlertCircle size={14} />{feedback}</span> : <span />}
          <button className="settings-button settings-button-secondary" type="button" disabled={saving || loadingLayer !== null} onClick={() => void clearActiveLayer()}><Trash2 size={15} />清空</button>
          <button className="settings-button settings-button-secondary" type="button" disabled={saving || loadingLayer !== null} onClick={() => void reloadActiveLayer()}><RefreshCw size={15} />重新载入</button>
          <button className="settings-button settings-button-primary" type="button" disabled={saving || !dirty || bytes > LIMITS[activeLayer]} onClick={() => void persistActiveLayer()}><Save size={15} />保存</button>
        </div>
        {activeLayer === "l2" && bytes >= LIMITS.l2 * 0.8 && bytes <= LIMITS.l2 ? <p className="memory-capacity-warning">L2 已使用超过 80%，建议整理或归档不再需要的内容。</p> : null}
        {bytes > LIMITS[activeLayer] ? <p className="memory-limit-error">内容超过当前层级的字节上限，请删减后再保存。</p> : null}
      </div>
    </section>
  );
}

function MemoryToggle({ label, description, checked, disabled, onChange }: { label: string; description: string; checked: boolean; disabled: boolean; onChange: (value: boolean) => void }) {
  return (
    <div className="settings-row">
      <div className="settings-row-copy"><strong>{label}</strong><span>{description}</span></div>
      <div className="settings-row-control">
        <button
          className={`settings-toggle${checked ? " settings-toggle-active" : ""}`}
          type="button"
          role="switch"
          aria-checked={checked}
          aria-label={label}
          disabled={disabled}
          onClick={() => onChange(!checked)}
        >
          <span />
        </button>
      </div>
    </div>
  );
}

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message;
  return typeof error === "string" ? error : fallback;
}
