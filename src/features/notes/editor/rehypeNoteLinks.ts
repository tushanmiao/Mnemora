type Node = { type: string; tagName?: string; value?: string; properties?: Record<string, unknown>; children?: Node[] };
const text = (node: Node): string => node.value ?? node.children?.map(text).join("") ?? "";
const slug = (value: string) => value.toLowerCase().trim().replace(/[^\p{L}\p{N}\s_-]/gu, "").replace(/\s/g, "-");

/** Keep offset-based outline IDs, while resolving normal #heading links. */
export function rehypeNoteLinks() {
  return (root: Node) => {
    const ids = new Map<string, string>(), counts = new Map<string, number>();
    const walk = (node: Node, visit: (node: Node) => void) => { visit(node); node.children?.forEach((child) => walk(child, visit)); };
    walk(root, (node) => {
      if (!node.tagName?.match(/^h[1-6]$/) || typeof node.properties?.id !== "string") return;
      const base = slug(text(node)), count = counts.get(base) ?? 0;
      counts.set(base, count + 1); ids.set(count ? `${base}-${count}` : base, node.properties.id);
    });
    walk(root, (node) => {
      const href = node.properties?.href;
      if (typeof href !== "string" || !href.startsWith("#")) return;
      let target = href.slice(1); try { target = decodeURIComponent(target); } catch { return; }
      if (ids.has(target)) node.properties!.href = `#${ids.get(target)}`;
    });
  };
}
