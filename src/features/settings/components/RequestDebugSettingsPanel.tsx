import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { AlertTriangle, Check, Copy, RefreshCw, Trash2 } from "lucide-react";
import type { AppSettings } from "../../../types/appSettings";
import type { RequestDebugRecord } from "../../../types/requestDebug";
import { clearRequestDebugRecords, loadRequestDebugRecords } from "../api/requestDebug";
import { MEMORY_DIAGNOSTICS_ENABLED } from "../../../runtime/buildFlags";
import "../styles/request-debug-settings.css";

const MemoryDiagnosticsPanel = MEMORY_DIAGNOSTICS_ENABLED
  ? lazy(() => import("../../diagnostics/memory/components/MemoryDiagnosticsPanel"))
  : null;

type Props = {
  settings: AppSettings;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
};

export function RequestDebugSettingsPanel({ settings, onSave }: Props) {
  const [records, setRecords] = useState<RequestDebugRecord[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const selected = records.find((record) => record.id === selectedId) ?? records[0] ?? null;

  const refresh = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const next = await loadRequestDebugRecords();
      setRecords(next);
      setSelectedId((current) => current && next.some((item) => item.id === current) ? current : next[0]?.id ?? null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const toggleEnabled = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSave({ ...settings, requestDebugEnabled: !settings.requestDebugEnabled });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    setBusy(true);
    setError(null);
    try {
      await clearRequestDebugRecords();
      setRecords([]);
      setSelectedId(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const copy = async () => {
    if (!selected) return;
    await navigator.clipboard.writeText(JSON.stringify(selected, null, 2));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_200);
  };

  return (
    <section className="settings-content debug-settings-content">
      {MemoryDiagnosticsPanel ? (
        <Suspense fallback={null}><MemoryDiagnosticsPanel /></Suspense>
      ) : null}
      <div className="settings-content-heading">
        <div><h2>请求调试</h2><span>检查实际请求结构和标准化响应</span></div>
        <div className="settings-heading-actions">
          <button className={`debug-enable-button${settings.requestDebugEnabled ? " debug-enable-button-active" : ""}`} type="button" role="switch" aria-checked={settings.requestDebugEnabled} disabled={busy} onClick={() => void toggleEnabled()}>
            <span />{settings.requestDebugEnabled ? "记录已开启" : "记录已关闭"}
          </button>
          <button className="settings-button settings-button-secondary" type="button" disabled={busy} onClick={() => void refresh()}><RefreshCw size={15} /><span>刷新</span></button>
          <button className="settings-button settings-button-secondary" type="button" disabled={busy || records.length === 0} onClick={() => void clear()}><Trash2 size={15} /><span>清空</span></button>
        </div>
      </div>

      <div className="debug-warning"><AlertTriangle size={17} /><span>调试记录仅保存在内存中，最多 30 条。认证信息会脱敏；请求正文仍可能包含你的对话内容。</span></div>
      {error ? <div className="settings-feedback settings-feedback-error"><span>{error}</span></div> : null}

      <div className="debug-workspace">
        <div className="debug-record-list">
          {records.map((record) => (
            <button className={`debug-record-item${selected?.id === record.id ? " debug-record-item-active" : ""}`} type="button" key={record.id} onClick={() => setSelectedId(record.id)}>
              <span className={`debug-record-status debug-record-status-${record.status}`} />
              <span><strong>{record.displayName}</strong><small>{formatTime(record.createdAtMs)} · {formatDuration(record.durationMs)}</small></span>
            </button>
          ))}
          {records.length === 0 ? <div className="debug-empty">{settings.requestDebugEnabled ? "发送模型请求后会在这里显示" : "开启记录后才会捕获新请求"}</div> : null}
        </div>

        <div className="debug-record-detail">
          {selected ? (
            <>
              <header><div><strong>{selected.providerName} · {selected.displayName}</strong><span>{selected.protocol} · {selected.status}</span></div><button className="icon-button" type="button" title="复制完整调试记录" onClick={() => void copy()}>{copied ? <Check size={16} /> : <Copy size={16} />}</button></header>
              <DebugBlock title={`${selected.request.method} ${selected.request.url}`} value={{ headers: selected.request.headers, body: selected.request.body }} truncated={selected.request.bodyTruncated} />
              <DebugBlock title={`响应${selected.response.statusCode ? ` · HTTP ${selected.response.statusCode}` : ""}`} value={selected.response.body ?? null} truncated={selected.response.bodyTruncated} />
            </>
          ) : <div className="debug-empty">选择一条请求查看详情</div>}
        </div>
      </div>
    </section>
  );
}

function DebugBlock({ title, value, truncated }: { title: string; value: unknown; truncated: boolean }) {
  return <section className="debug-json-block"><div><h3>{title}</h3>{truncated ? <span>内容已截断</span> : null}</div><pre>{JSON.stringify(value, null, 2)}</pre></section>;
}

function formatTime(value: number) {
  return new Intl.DateTimeFormat("zh-CN", { hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(value);
}

function formatDuration(value: number) {
  return value < 1_000 ? `${value} ms` : `${(value / 1_000).toFixed(1)} s`;
}
