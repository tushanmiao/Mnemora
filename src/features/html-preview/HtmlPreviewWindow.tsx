import { useEffect, useState } from "react";
import { AlertCircle, LoaderCircle } from "lucide-react";
import { loadHtmlPreview } from "./api";
import { buildSandboxDocument } from "./sandboxDocument";
import "./html-preview.css";

function previewTokenFromHash() {
  const prefix = "#html-preview/";
  if (!window.location.hash.startsWith(prefix)) return "";
  return decodeURIComponent(window.location.hash.slice(prefix.length));
}

export default function HtmlPreviewWindow() {
  const [document, setDocument] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    const token = previewTokenFromHash();
    if (!token) {
      setError("HTML 预览标识无效");
      return;
    }

    let cancelled = false;
    void loadHtmlPreview(token)
      .then((html) => {
        if (!cancelled) setDocument(buildSandboxDocument(html));
      })
      .catch((reason) => {
        if (!cancelled) {
          setError(reason instanceof Error ? reason.message : "HTML 预览加载失败");
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) {
    return (
      <main className="html-preview-state" role="alert">
        <AlertCircle size={20} />
        <span>{error}</span>
      </main>
    );
  }

  if (!document) {
    return (
      <main className="html-preview-state" role="status">
        <LoaderCircle className="html-preview-spin" size={20} />
        <span>正在准备预览</span>
      </main>
    );
  }

  return (
    <main className="html-preview-shell">
      <iframe
        className="html-preview-frame"
        title="HTML 预览内容"
        sandbox=""
        referrerPolicy="no-referrer"
        srcDoc={document}
      />
    </main>
  );
}

