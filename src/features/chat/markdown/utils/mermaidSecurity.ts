const DANGEROUS_SVG_TAGS = new Set(["script", "foreignobject", "iframe", "object", "embed", "image"]);
const MERMAID_NODE_SHAPES = "rect, polygon, circle, ellipse, path";
const MERMAID_PALETTE_SIZE = 6;

/** Mermaid 输出由受信任的渲染器生成，但仍移除脚本、外链和事件属性。 */
export function sanitizeMermaidSvg(svg: string) {
  if (typeof DOMParser === "undefined" || typeof document === "undefined") return svg;
  const parsed = new DOMParser().parseFromString(svg, "image/svg+xml");
  const root = parsed.documentElement;
  if (!root || root.tagName.toLowerCase() !== "svg") throw new Error("Mermaid 未生成有效 SVG");

  for (const element of Array.from(root.querySelectorAll("*"))) {
    if (DANGEROUS_SVG_TAGS.has(element.tagName.toLowerCase())) {
      element.remove();
      continue;
    }
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim().toLowerCase();
      if (name.startsWith("on") || name === "srcdoc") {
        element.removeAttribute(attribute.name);
      } else if ((name === "href" || name === "xlink:href") && !value.startsWith("#")) {
        element.removeAttribute(attribute.name);
      }
    }
  }

  markDefaultFlowchartNodes(root);
  root.setAttribute("role", "img");
  root.removeAttribute("aria-roledescription");
  return new XMLSerializer().serializeToString(root);
}

/**
 * Mermaid 11 把默认主题色主要放在 SVG 内嵌样式表中，而 classDef/style
 * 通常会写成 shape 的 inline style。仅依赖 `[fill="#..."]` 无法稳定命中
 * 默认节点；使用全局 `!important` 又会抹掉用户定义的语义颜色。
 *
 * 因此这里只给“没有额外 class、没有 inline fill/stroke”的默认流程图节点
 * 标记色板序号。实际颜色仍由应用 CSS 根据明暗主题决定。用户 classDef、
 * `style A ...`、透明辅助路径和标签背景都不会被标记。
 */
function markDefaultFlowchartNodes(root: Element) {
  let paletteIndex = 0;
  for (const group of Array.from(root.querySelectorAll("g.node"))) {
    const classes = Array.from(group.classList);
    if (!classes.includes("default") || classes.some((name) => name !== "node" && name !== "default")) {
      continue;
    }

    const shapes = Array.from(group.querySelectorAll(MERMAID_NODE_SHAPES)).filter((shape) => {
      if (shape.closest(".label, marker, defs, clipPath")) return false;
      return shape.getAttribute("fill")?.trim().toLowerCase() !== "none";
    });
    if (shapes.length === 0 || shapes.some(hasAuthoredPaint)) continue;

    for (const shape of shapes) {
      shape.setAttribute("data-mnemora-node-tone", String(paletteIndex));
    }
    paletteIndex = (paletteIndex + 1) % MERMAID_PALETTE_SIZE;
  }
}

function hasAuthoredPaint(element: Element) {
  const style = element.getAttribute("style")?.toLowerCase() ?? "";
  return /(?:^|;)\s*(?:fill|stroke)\s*:/.test(style);
}

export function mermaidThemeConfig(host: HTMLElement) {
  const shell = host.closest<HTMLElement>(".app-shell") ?? host;
  const styles = getComputedStyle(shell);
  const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  const readColor = (name: string, fallback: string) => resolveMermaidColor(shell, name, fallback);
  const dark = shell.getAttribute("data-theme") === "dark";
  return {
    theme: dark ? "dark" as const : "base" as const,
    securityLevel: "strict" as const,
    startOnLoad: false,
    htmlLabels: false,
    suppressErrorRendering: true,
    themeVariables: {
      background: readColor("--color-surface", dark ? "#1d2024" : "#ffffff"),
      primaryColor: dark ? "#263f55" : "#dcecff",
      primaryTextColor: readColor("--color-text", dark ? "#edf0f2" : "#202427"),
      textColor: readColor("--color-text", dark ? "#edf0f2" : "#202427"),
      primaryBorderColor: dark ? "#75b5e8" : "#397eb8",
      nodeBkg: dark ? "#263f55" : "#dcecff",
      nodeBorder: dark ? "#75b5e8" : "#397eb8",
      labelBackground: readColor("--color-surface-raised", dark ? "#282d32" : "#ffffff"),
      lineColor: readColor("--color-muted", dark ? "#adb7bc" : "#687276"),
      secondaryColor: dark ? "#3f3150" : "#f1e3ff",
      secondaryBorderColor: dark ? "#c59be3" : "#8553a5",
      tertiaryColor: dark ? "#2f4939" : "#dff3e5",
      tertiaryBorderColor: dark ? "#85c69a" : "#4d8f62",
      clusterBkg: dark ? "#252a31" : "#f7f8fc",
      clusterBorder: readColor("--color-border", dark ? "#626a74" : "#bcc3cc"),
      noteBkgColor: dark ? "#55452b" : "#fff1c7",
      noteBorderColor: dark ? "#e2bd68" : "#a97916",
      noteTextColor: readColor("--color-text", dark ? "#edf0f2" : "#202427"),
      actorBkg: dark ? "#4d303d" : "#fde1eb",
      actorBorder: dark ? "#e18aaf" : "#a53e6b",
      actorTextColor: readColor("--color-text", dark ? "#edf0f2" : "#202427"),
      signalColor: readColor("--color-muted", dark ? "#adb7bc" : "#687276"),
      fontFamily: read("--reading-font-family", "system-ui, sans-serif"),
    },
  };
}

/**
 * Mermaid's color parser (khroma) accepts concrete RGB/HEX/HSL values, but
 * not CSS Color 4 expressions such as `color-mix(...)`. The application theme
 * intentionally uses those expressions for hover/active surfaces, so resolve
 * the custom property through the browser before handing it to Mermaid.
 */
function resolveMermaidColor(host: HTMLElement, property: string, fallback: string) {
  if (typeof document === "undefined" || typeof getComputedStyle === "undefined") return fallback;

  const probe = document.createElement("span");
  probe.setAttribute("aria-hidden", "true");
  probe.style.position = "absolute";
  probe.style.width = "0";
  probe.style.height = "0";
  probe.style.overflow = "hidden";
  // Referencing the custom property keeps nested `var(...)` values inherited
  // from the theme host instead of trying to parse them in JavaScript.
  probe.style.color = `var(${property}, ${fallback})`;
  host.appendChild(probe);
  const resolved = getComputedStyle(probe).color.trim();
  probe.remove();
  return isMermaidColor(resolved) ? resolved : fallback;
}

function isMermaidColor(value: string) {
  const normalized = value.trim().toLowerCase();
  return /^#[0-9a-f]{3,8}$/.test(normalized)
    || /^(?:rgb|rgba|hsl|hsla)\(/.test(normalized)
    || /^(?:transparent|black|white|red|green|blue|yellow|cyan|magenta|gray|grey)$/.test(normalized);
}
