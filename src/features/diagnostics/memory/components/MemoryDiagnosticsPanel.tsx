import { save } from "@tauri-apps/plugin-dialog";
import { getVersion } from "@tauri-apps/api/app";
import { Activity, Download, Pause, Play, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { exportMemoryDiagnostics, sampleMemoryProcessTree } from "../api";
import { samplePageMemory } from "../pageSample";
import type { MemoryTimelineSample } from "../types";
import "../styles/memory-diagnostics.css";

const MAX_TIMELINE_SAMPLES = 24;
const SAMPLE_INTERVAL_MS = 5_000;

export default function MemoryDiagnosticsPanel() {
  const [scene, setScene] = useState("settings");
  const [running, setRunning] = useState(true);
  const [samples, setSamples] = useState<MemoryTimelineSample[]>([]);
  const [error, setError] = useState("");
  const samplingRef = useRef(false);
  const latest = samples[samples.length - 1] ?? null;

  const sample = useCallback(async () => {
    if (samplingRef.current) return;
    samplingRef.current = true;
    try {
      const process = await sampleMemoryProcessTree();
      const next = { scene, process, page: samplePageMemory() };
      setSamples((current) => [...current, next].slice(-MAX_TIMELINE_SAMPLES));
      setError("");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      samplingRef.current = false;
    }
  }, [scene]);

  useEffect(() => {
    if (!running) return;
    void sample();
    const timer = window.setInterval(() => void sample(), SAMPLE_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [running, sample]);

  const exportReport = async () => {
    if (samples.length === 0) return;
    const path = await save({
      title: "导出内存诊断报告",
      defaultPath: `mnemora-memory-${Date.now()}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) return;
    await exportMemoryDiagnostics(path, {
      schemaVersion: 1,
      appVersion: await getVersion(),
      exportedAtMs: Date.now(),
      samples,
    });
  };

  return (
    <section className="memory-diagnostics" aria-label="内存诊断">
      <header>
        <div><h2><Activity size={17} />内存诊断</h2><span>开发构建 · 每 5 秒低频采样</span></div>
        <div className="memory-diagnostics-actions">
          <select value={scene} aria-label="诊断场景" onChange={(event) => setScene(event.target.value)}>
            <option value="idle">空闲</option><option value="chat">Chat</option><option value="pdf">PDF</option><option value="english">English</option><option value="settings">设置</option>
          </select>
          <button className="icon-button" type="button" title={running ? "暂停采样" : "继续采样"} aria-label={running ? "暂停采样" : "继续采样"} onClick={() => setRunning((value) => !value)}>{running ? <Pause size={15} /> : <Play size={15} />}</button>
          <button className="icon-button" type="button" title="立即采样" aria-label="立即采样" onClick={() => void sample()}><RefreshCw size={15} /></button>
          <button className="icon-button" type="button" title="导出脱敏报告" aria-label="导出脱敏报告" disabled={samples.length === 0} onClick={() => void exportReport()}><Download size={15} /></button>
        </div>
      </header>
      {error ? <p className="memory-diagnostics-error">{error}</p> : null}
      {latest ? (
        <>
          <div className="memory-diagnostics-summary">
            <Metric label="进程私有内存" value={formatBytes(latest.process.totalPrivateBytes)} />
            <Metric label="进程工作集" value={formatBytes(latest.process.totalWorkingSetBytes)} />
            <Metric label="JS Heap" value={formatBytes(latest.page.jsHeapUsedBytes)} />
            <Metric label="Canvas" value={`${latest.page.canvasCount} · ${formatBytes(latest.page.canvasEstimatedBytes)}`} />
            <Metric label="DOM" value={latest.page.domNodes.toLocaleString()} />
            <Metric label="注册资源" value={`${latest.page.registry.count} · ${formatBytes(latest.page.registry.estimatedBytes)}`} />
          </div>
          <div className="memory-process-list">
            {latest.process.processes.map((process) => (
              <div key={process.pid}><span><strong>{process.role}</strong><small>PID {process.pid}</small></span><span>{formatBytes(process.privateBytes)}<small>{formatBytes(process.workingSetBytes)} WS</small></span></div>
            ))}
          </div>
        </>
      ) : <p className="memory-diagnostics-empty">等待首个样本</p>}
    </section>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div><span>{label}</span><strong>{value}</strong></div>;
}

function formatBytes(value: number | null) {
  if (value === null) return "不可用";
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
