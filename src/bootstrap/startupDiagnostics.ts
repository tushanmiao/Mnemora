export type StartupStage = "bootstrap" | "import-window" | "render-window" | "ready";

export type StartupDiagnostic = {
  stage: StartupStage;
  context?: string;
  name: string;
  message: string;
  stack?: string;
  componentStack?: string;
  occurredAt: string;
};

type StartupDiagnosticDetails = {
  context?: string;
  componentStack?: string;
};

let currentStage: StartupStage = "bootstrap";
let latestDiagnostic: StartupDiagnostic | null = null;
let lastPersistedSignature = "";
let lastPersistedAt = 0;

const SECRET_PATTERN = /(api[-_ ]?key|authorization|bearer|password|token)\s*[:=]\s*[^\s,;]+|sk-[A-Za-z0-9_-]{12,}/gi;

function bounded(value: string, limit: number) {
  const redacted = value.replace(SECRET_PATTERN, "$1=[已隐藏]");
  return redacted.length <= limit ? redacted : `${redacted.slice(0, limit)}...`;
}

export function setStartupStage(stage: StartupStage) {
  currentStage = stage;
}

export function createStartupDiagnostic(
  reason: unknown,
  details: StartupDiagnosticDetails = {},
): StartupDiagnostic {
  const error = reason instanceof Error
    ? reason
    : new Error(typeof reason === "string" ? reason : "未知启动错误");
  const diagnostic: StartupDiagnostic = {
    stage: currentStage,
    context: details.context ? bounded(details.context, 200) : undefined,
    name: bounded(error.name || "Error", 80),
    message: bounded(error.message || "未知启动错误", 1_000),
    stack: error.stack ? bounded(error.stack, 8_000) : undefined,
    componentStack: details.componentStack ? bounded(details.componentStack, 4_000) : undefined,
    occurredAt: new Date().toISOString(),
  };
  latestDiagnostic = diagnostic;
  return diagnostic;
}

/** 持久化失败只写入控制台，不能再次触发根级未处理 rejection。 */
export function recordStartupDiagnostic(diagnostic: StartupDiagnostic) {
  latestDiagnostic = diagnostic;
  const signature = [diagnostic.stage, diagnostic.context, diagnostic.name, diagnostic.message].join("|");
  const now = Date.now();
  if (signature === lastPersistedSignature && now - lastPersistedAt < 1_000) return;
  lastPersistedSignature = signature;
  lastPersistedAt = now;
  void import("@tauri-apps/api/core")
    .then(({ invoke, isTauri }) => (
      isTauri() ? invoke<void>("record_startup_error", { diagnostic }) : undefined
    ))
    .catch((error) => console.warn("Mnemora 启动诊断写入失败", error));
}

function captureStartupDiagnostic(reason: unknown, details?: StartupDiagnosticDetails) {
  const diagnostic = createStartupDiagnostic(reason, details);
  recordStartupDiagnostic(diagnostic);
  return diagnostic;
}

export function getLatestStartupDiagnostic() {
  return latestDiagnostic;
}

export function installGlobalErrorCapture() {
  const onError = (event: ErrorEvent) => {
    const diagnostic = captureStartupDiagnostic(event.error ?? event.message);
    console.error("Mnemora 前端异常", diagnostic);
  };
  const onUnhandledRejection = (event: PromiseRejectionEvent) => {
    const diagnostic = captureStartupDiagnostic(event.reason);
    console.error("Mnemora 未处理的异步异常", diagnostic);
  };
  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onUnhandledRejection);
  return () => {
    window.removeEventListener("error", onError);
    window.removeEventListener("unhandledrejection", onUnhandledRejection);
  };
}

export function diagnosticText(diagnostic: StartupDiagnostic) {
  return [
    `Mnemora startup failure`,
    `stage: ${diagnostic.stage}`,
    diagnostic.context ? `context: ${diagnostic.context}` : "",
    `time: ${diagnostic.occurredAt}`,
    `error: ${diagnostic.name}: ${diagnostic.message}`,
    diagnostic.stack ? `stack:\n${diagnostic.stack}` : "",
    diagnostic.componentStack ? `component stack:\n${diagnostic.componentStack}` : "",
  ].filter(Boolean).join("\n");
}
