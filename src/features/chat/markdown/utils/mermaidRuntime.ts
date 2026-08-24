import type { MermaidConfig } from "mermaid";

type MermaidModule = typeof import("mermaid");

let modulePromise: Promise<MermaidModule> | undefined;
let renderQueue: Promise<void> = Promise.resolve();
const FONT_READY_TIMEOUT_MS = 1_500;

function loadMermaid() {
  modulePromise ??= import("mermaid");
  return modulePromise;
}

/** Mermaid uses process-wide configuration, so initialize/parse/render must be
 * one serialized operation. This prevents concurrent diagrams from swapping
 * light/dark variables while another SVG is being produced. */
export function renderMermaid(code: string, id: string, config: MermaidConfig) {
  const task = renderQueue.then(async () => {
    const { default: mermaid } = await loadMermaid();
    mermaid.initialize(config);
    await waitForMermaidFonts(config);
    await mermaid.parse(code, { suppressErrors: false });
    const rendered = await mermaid.render(id, code);
    return {
      ...rendered,
      // Keep the narrow compatibility repair while leaving dimensions and the
      // original viewBox entirely under Mermaid's control.
      svg: rendered.svg.replace(/translate\(undefined,\s*NaN\)/g, "translate(0, 0)"),
    };
  });
  renderQueue = task.then(() => undefined, () => undefined);
  return task;
}

/** Wait for the exact diagram font without allowing a broken web font to stall
 * the serialized render queue indefinitely. */
export async function waitForMermaidFonts(config: MermaidConfig) {
  const fonts = document.fonts;
  if (!fonts) return;

  const themeVariables = config.themeVariables as Record<string, unknown> | undefined;
  const fontFamily = typeof themeVariables?.fontFamily === "string"
    ? themeVariables.fontFamily
    : "system-ui, sans-serif";
  const configuredSize = themeVariables?.fontSize ?? config.fontSize ?? "13px";
  const fontSize = typeof configuredSize === "number" ? `${configuredSize}px` : String(configuredSize);

  await new Promise<void>((resolve) => {
    let settled = false;
    const finish = () => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      resolve();
    };
    const timer = window.setTimeout(finish, FONT_READY_TIMEOUT_MS);
    void Promise.allSettled([
      fonts.ready,
      fonts.load(`${fontSize} ${fontFamily}`, "数据库 Database 表 Table"),
    ]).then(finish);
  });
}
