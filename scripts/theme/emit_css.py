#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成 src/styles/themes.css 与设置页预览控件所需的色值。

用法： python3 scripts/theme/emit_css.py
"""
import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generate_themes as G
import presets as P

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))

ALIASES = """/* Compatibility aliases keep feature CSS stable while the palette stays token-driven. */
.app-shell {
  --color-app: var(--surface-app);
  --color-surface: var(--surface-content);
  --color-surface-raised: var(--surface-raised);
  --color-sidebar: var(--surface-sidebar);
  --color-rail: var(--surface-rail);
  --color-border: var(--border-default);
  --color-border-soft: var(--border-subtle);
  --color-text: var(--text-primary);
  --color-text-secondary: var(--text-secondary);
  --color-muted: var(--text-muted);
  --color-faint: var(--text-faint);
  --color-hover: color-mix(in srgb, var(--workspace-accent-soft) 34%, var(--surface-hover));
  --color-active: color-mix(in srgb, var(--workspace-accent-soft) 68%, var(--surface-selected));
  --color-accent: var(--workspace-accent);
  --color-accent-hover: var(--workspace-accent-strong);
  --color-accent-soft: var(--workspace-accent-soft);
  --color-on-accent: var(--on-accent);
  --color-danger: var(--status-danger);
  --color-danger-soft: var(--status-danger-soft);
  --color-danger-border: var(--status-danger-border);
  --color-success: var(--status-success);
  --color-success-soft: var(--status-success-soft);
  --color-success-border: var(--status-success-border);
  --color-warning: var(--status-warning);
  --color-warning-soft: var(--status-warning-soft);
  --color-warning-border: var(--status-warning-border);
  --color-info: var(--status-info);
  --color-info-soft: var(--status-info-soft);
  --color-info-border: var(--status-info-border);
  --color-user-bubble: color-mix(in srgb, var(--workspace-chat-soft) 70%, var(--surface-user));
  --app-font-size: var(--reading-font-size);
}
"""

HEADER = """/* 由 scripts/theme/emit_css.py 生成，请勿手改；改规格在 scripts/theme/presets.py。
 *
 * 每个预设都是一整套调色：表面、文字、边界、六个工作区身份色，以及材质
 * （圆角与阴影性格）。身份色遵循「一主五从」语法——以主导色相为轴，五个
 * 从属色按不均匀的色相偏移排布，并在明度与彩度上分层，因此区分度来自
 * 色相 + 明度 + 彩度三个维度，而不是把色相环等分成六份。
 */
"""


def main():
    chunks = [HEADER]
    for name in P.PRESET_ORDER:
        spec = P.PRESETS[name]
        chunks.append(f"/* ---- {name} ({spec['group']}) ---- */\n" + G.emit(name, spec))
    chunks.append(ALIASES)
    css = "\n\n".join(chunks) + "\n"
    out = os.path.join(ROOT, "src", "styles", "themes.css")
    with open(out, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(css)

    preview = {}
    for name in P.PRESET_ORDER:
        spec = P.PRESETS[name]
        light, dark = G.build(spec, False), G.build(spec, True)
        preview[name] = {
            "group": spec["group"],
            "radius": spec["material"]["light"].get("radius-panel", "7px"),
            "light": {k: light[k] for k in
                      ["surface-app", "surface-content", "surface-raised", "surface-sidebar",
                       "surface-rail", "border-default", "text-primary", "text-muted"]
                      + [f"workspace-{r}" for r in G.ORDER]},
            "dark": {k: dark[k] for k in
                     ["surface-app", "surface-content", "surface-raised", "surface-sidebar",
                      "surface-rail", "border-default", "text-primary", "text-muted"]
                     + [f"workspace-{r}" for r in G.ORDER]},
        }
    with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "preview.json"),
              "w", encoding="utf-8") as fh:
        json.dump(preview, fh, ensure_ascii=False, indent=1)

    # 设置页的预览色块与主题同源，避免二者各写一份而慢慢对不上。
    keys = ["surface-app", "surface-content", "surface-raised", "surface-sidebar", "surface-rail",
            "border-default", "text-primary", "text-muted"] + [f"workspace-{r}" for r in G.ORDER]

    def alias(key):
        return ("--preset-" + key.replace("surface-", "").replace("workspace-", "")
                .replace("border-default", "border").replace("text-", "text-"))

    blocks = []
    for name in P.PRESET_ORDER:
        spec = P.PRESETS[name]
        radius = spec["material"]["light"].get("radius-panel", "7px")
        for dark, sel in ((False, f'[data-theme-preset-preview="{name}"]'),
                          (True, f'.app-shell[data-theme="dark"] [data-theme-preset-preview="{name}"]')):
            t = G.build(spec, dark)
            body = "\n".join(f"  {alias(k)}: {t[k]};" for k in keys)
            blocks.append(f"{sel} {{\n{body}\n  --preset-radius: {radius};\n}}")
    preview_css = ("/* 由 scripts/theme/emit_css.py 生成，请勿手改。 */\n\n"
                   + "\n\n".join(blocks) + "\n")
    with open(os.path.join(ROOT, "src", "features", "settings", "styles",
                           "theme-preview.generated.css"), "w", encoding="utf-8", newline="\n") as fh:
        fh.write(preview_css)

    problems = []
    for name in P.PRESET_ORDER:
        problems += G.audit(name, P.PRESETS[name])
    print(f"写入 {out}（{len(css.splitlines())} 行，{len(P.PRESET_ORDER)} 套预设）")
    print("对比度体检:", f"{len(problems)} 项未达标" if problems else "全部通过")
    for p in problems:
        print("  ✗", p)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
