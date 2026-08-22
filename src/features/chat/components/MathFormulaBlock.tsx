import { useEffect, useRef, useState, type ReactNode } from "react";
import { Check, Copy, X } from "lucide-react";

type MathView = "rendered" | "latex";
type CopyState = "idle" | "copied" | "error";

export function MathFormulaBlock({ latex, children }: { latex: string; children: ReactNode }) {
  const [view, setView] = useState<MathView>("rendered");
  const [copyState, setCopyState] = useState<CopyState>("idle");
  const resetTimerRef = useRef<number | null>(null);

  useEffect(() => () => {
    if (resetTimerRef.current !== null) window.clearTimeout(resetTimerRef.current);
  }, []);

  const copyLatex = async () => {
    try {
      await copyText(latex);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
    if (resetTimerRef.current !== null) window.clearTimeout(resetTimerRef.current);
    resetTimerRef.current = window.setTimeout(() => setCopyState("idle"), 1_600);
  };

  const copyLabel = copyState === "copied"
    ? "LaTeX 已复制"
    : copyState === "error"
      ? "复制 LaTeX 失败，点击重试"
      : "复制 LaTeX";

  return (
    <div className="markdown-math-block" data-view={view}>
      <div className="markdown-code-toolbar markdown-math-toolbar">
        <span>数学公式</span>
        <div className="markdown-code-actions">
          <div className="markdown-math-view-switch" role="group" aria-label="公式显示方式">
            <button
              type="button"
              aria-pressed={view === "rendered"}
              onClick={() => setView("rendered")}
            >
              渲染
            </button>
            <button
              type="button"
              aria-pressed={view === "latex"}
              onClick={() => setView("latex")}
            >
              LaTeX
            </button>
          </div>
          <button
            className={`markdown-copy-button markdown-copy-button-${copyState}`}
            type="button"
            title={copyLabel}
            aria-label={copyLabel}
            aria-live="polite"
            onClick={() => void copyLatex()}
          >
            {copyState === "copied" ? <Check size={15} /> : null}
            {copyState === "error" ? <X size={15} /> : null}
            {copyState === "idle" ? <Copy size={15} /> : null}
          </button>
        </div>
      </div>
      <div className="markdown-math-rendered" hidden={view !== "rendered"}>
        {children}
      </div>
      <pre className="markdown-math-source" hidden={view !== "latex"}><code>{latex}</code></pre>
    </div>
  );
}

async function copyText(value: string) {
  if (navigator.clipboard) {
    await navigator.clipboard.writeText(value);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();
  if (!copied) throw new Error("复制失败");
}
