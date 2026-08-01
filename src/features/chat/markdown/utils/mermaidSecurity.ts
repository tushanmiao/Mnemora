const DANGEROUS_SVG_TAGS = new Set(["script", "foreignobject", "iframe", "object", "embed", "image"]);

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

  root.setAttribute("role", "img");
  root.removeAttribute("aria-roledescription");
  return new XMLSerializer().serializeToString(root);
}

export function mermaidThemeConfig(host: HTMLElement) {
  const shell = host.closest<HTMLElement>(".app-shell") ?? host;
  const styles = getComputedStyle(shell);
  const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  const dark = shell.getAttribute("data-theme") === "dark";
  return {
    theme: dark ? "dark" as const : "base" as const,
    securityLevel: "strict" as const,
    startOnLoad: false,
    htmlLabels: false,
    suppressErrorRendering: true,
    themeVariables: {
      background: read("--color-surface", dark ? "#1d2024" : "#ffffff"),
      primaryColor: read("--color-accent-soft", dark ? "#263b3e" : "#e6f1ef"),
      primaryTextColor: read("--color-text", dark ? "#edf0f2" : "#202427"),
      primaryBorderColor: read("--color-accent", "#3b8581"),
      lineColor: read("--color-muted", dark ? "#adb7bc" : "#687276"),
      secondaryColor: read("--color-surface-raised", dark ? "#282d32" : "#f7f8f8"),
      tertiaryColor: read("--color-hover", dark ? "#323940" : "#f0f3f2"),
      fontFamily: read("--reading-font-family", "system-ui, sans-serif"),
    },
  };
}
