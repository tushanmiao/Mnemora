import { useState } from "react";
import { Copy, RefreshCw } from "lucide-react";
import type { StartupDiagnostic } from "./startupDiagnostics";
import { diagnosticText } from "./startupDiagnostics";

type Props = {
  diagnostic: StartupDiagnostic;
  title?: string;
};

export function StartupFailure({ diagnostic, title = "Mnemora 启动失败" }: Props) {
  const [copied, setCopied] = useState(false);

  const copyDiagnostic = async () => {
    try {
      await navigator.clipboard.writeText(diagnosticText(diagnostic));
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };

  return (
    <main className="startup-failure" role="alert">
      <div className="startup-failure-panel">
        <span className="startup-failure-brand">Mnemora</span>
        <h1>{title}</h1>
        <p>界面遇到错误，但应用仍在运行。可以重新加载，或复制诊断信息用于排查。</p>
        <code>{diagnostic.stage} · {diagnostic.name}: {diagnostic.message}</code>
        <div className="startup-failure-actions">
          <button type="button" onClick={() => window.location.reload()}>
            <RefreshCw size={16} />重新加载
          </button>
          <button type="button" onClick={() => void copyDiagnostic()}>
            <Copy size={16} />{copied ? "已复制" : "复制诊断"}
          </button>
        </div>
      </div>
    </main>
  );
}
