#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Mnemora 主题生成器。

设计语法（所有预设共享，这是修掉"图表默认色"观感的关键）：
  - 六个工作区身份色不再等间距均分色相环，而是「一主五从」：
    以 anchor 色相为主，五个从属色按 **不均匀** 的色相偏移排布，
    同时在明度与彩度上拉开层级（彩度极差约 3.5 倍，旧方案只有 1.2 倍）。
  - 区分度由「色相 + 明度 + 彩度」三个维度共同承担，而不是只靠色相。
所有颜色在 OKLCH 里定义，落回 sRGB 时按需降彩度以保证在色域内。
"""
import math, json, sys

# ---------- 色彩基础 ----------
def _lin(c): return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
def _gam(c): return 12.92 * c if c <= 0.0031308 else 1.055 * (c ** (1 / 2.4)) - 0.055

def oklch_to_rgb(L, C, H):
    h = math.radians(H)
    a, b = C * math.cos(h), C * math.sin(h)
    l_ = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m_ = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s_ = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3
    r = 4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_
    g = -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_
    bl = -0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_
    return r, g, bl

def in_gamut(L, C, H):
    return all(-1e-4 <= v <= 1 + 1e-4 for v in oklch_to_rgb(L, C, H))

def hex_of(L, C, H):
    """超出 sRGB 色域时二分降彩度，保住明度与色相。"""
    if not in_gamut(L, C, H):
        lo, hi = 0.0, C
        for _ in range(40):
            mid = (lo + hi) / 2
            if in_gamut(L, mid, H): lo = mid
            else: hi = mid
        C = lo
    r, g, b = oklch_to_rgb(L, C, H)
    out = []
    for v in (r, g, b):
        v = min(1.0, max(0.0, v))
        out.append(round(_gam(v) * 255))
    return "#{:02x}{:02x}{:02x}".format(*out)

def rel_lum(hexstr):
    h = hexstr.lstrip("#")
    r, g, b = [_lin(int(h[i:i + 2], 16) / 255) for i in (0, 2, 4)]
    return 0.2126 * r + 0.7152 * g + 0.0722 * b

def contrast(a, b):
    la, lb = rel_lum(a), rel_lum(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)

# ---------- 角色语法 ----------
# (相对 anchor 的色相偏移, 明度, 彩度倍率)。偏移刻意不等距。
ROLES_LIGHT = {
    "chat":     (0,    0.505, 1.00),   # 主导：应用重心，彩度最高
    "notes":    (+26,  0.455, 0.88),   # 与主导同族，靠明度拉开
    "overview": (+74,  0.522, 0.66),   # 暖、亮
    "work":     (-40,  0.448, 0.82),   # 冷、沉
    "english":  (+140, 0.518, 0.54),   # 离主导最远
    "settings": (-98,  0.500, 0.26),   # 近中性，刻意后退
}
ROLES_DARK = {
    "chat":     (0,    0.790, 0.82),
    "notes":    (+26,  0.748, 0.76),
    "overview": (+74,  0.828, 0.62),
    "work":     (-40,  0.742, 0.72),
    "english":  (+140, 0.806, 0.50),
    "settings": (-98,  0.772, 0.28),
}
ORDER = ["overview", "chat", "notes", "work", "english", "settings"]


def build(spec, dark=False):
    """由紧凑规格展开成完整 token 表。"""
    sh = spec["surface_hue"]
    sc = spec["surface_chroma"] * (spec.get("dark_surface_chroma", 1.0) if dark else 1.0)
    anchor = spec["anchor"]
    cscale = spec.get("chroma", 1.0)
    t = {}

    if not dark:
        lad = spec.get("light_ladder", (0.955, 0.988, 1.000, 0.932, 0.921, 0.888, 0.945))
        t["surface-app"]      = hex_of(lad[0], sc, sh)
        t["surface-content"]  = hex_of(lad[1], sc * 0.45, sh)
        t["surface-raised"]   = hex_of(lad[2], sc * 0.25, sh)
        t["surface-sidebar"]  = hex_of(lad[3], sc * 1.15, sh)
        t["surface-hover"]    = hex_of(lad[4], sc * 1.25, sh)
        t["surface-selected"] = hex_of(lad[5], sc * 1.7, sh)
        t["surface-user"]     = hex_of(lad[6], sc * 1.3, anchor)
        t["surface-rail"]     = hex_of(*spec["rail"])
        t["text-primary"]     = hex_of(0.255, min(0.030, sc * 1.6), sh)
        t["text-secondary"]   = hex_of(0.430, min(0.024, sc * 1.3), sh)
        t["text-muted"]       = hex_of(0.540, min(0.020, sc * 1.1), sh)
        t["text-faint"]       = hex_of(0.672, min(0.018, sc), sh)
        t["border-default"]   = hex_of(spec.get("border_l", 0.850), sc * 1.6, sh)
        t["border-subtle"]    = hex_of(spec.get("border_l", 0.850) + 0.072, sc * 1.15, sh)
        roles, soft_l, soft_c, strong_d = ROLES_LIGHT, 0.947, 0.30, -0.095
    else:
        lad = spec.get("dark_ladder", (0.205, 0.262, 0.310, 0.238, 0.340, 0.398, 0.290))
        t["surface-app"]      = hex_of(lad[0], sc * 1.7, sh)
        t["surface-content"]  = hex_of(lad[1], sc * 1.5, sh)
        t["surface-raised"]   = hex_of(lad[2], sc * 1.3, sh)
        t["surface-sidebar"]  = hex_of(lad[3], sc * 1.6, sh)
        t["surface-hover"]    = hex_of(lad[4], sc * 1.4, sh)
        t["surface-selected"] = hex_of(lad[5], sc * 1.5, sh)
        t["surface-user"]     = hex_of(lad[6], sc * 1.6, anchor)
        t["surface-rail"]     = hex_of(max(0.120, lad[0] - 0.075), sc * 1.8, sh)
        t["text-primary"]     = hex_of(0.968, min(0.014, sc), sh)
        t["text-secondary"]   = hex_of(0.850, min(0.018, sc * 1.2), sh)
        t["text-muted"]       = hex_of(0.720, min(0.020, sc * 1.2), sh)
        t["text-faint"]       = hex_of(0.605, min(0.020, sc * 1.2), sh)
        t["border-default"]   = hex_of(0.415, sc * 2.0, sh)
        t["border-subtle"]    = hex_of(0.330, sc * 1.8, sh)
        roles, soft_l, soft_c, strong_d = ROLES_DARK, 0.302, 0.55, +0.085

    base_c = spec["role_chroma"] * cscale
    lshift = spec.get("role_l_shift", 0.0) * (-1 if dark else 1)
    for role, (dh, L, cm) in roles.items():
        H = (anchor + dh * spec.get("spread", 1.0)) % 360
        C = base_c * cm
        L = L + lshift
        t[f"workspace-{role}"] = hex_of(L, C, H)
        t[f"workspace-{role}-soft"] = hex_of(soft_l, C * soft_c, H)
        t[f"workspace-{role}-strong"] = hex_of(L + strong_d, C * (1.05 if not dark else 0.9), H)
    return t


def emit(name, spec):
    """输出一个预设的明暗两段 CSS。"""
    out = []
    for dark in (False, True):
        t = build(spec, dark)
        sel = (f'.app-shell[data-theme="dark"][data-theme-preset="{name}"]' if dark
               else f'.app-shell[data-theme-preset="{name}"]')
        lines = [f"{sel} {{"]
        for k in ["surface-app", "surface-content", "surface-raised", "surface-sidebar",
                  "surface-rail", "surface-hover", "surface-selected", "surface-user"]:
            lines.append(f"  --{k}: {t[k]};")
        for k in ["text-primary", "text-secondary", "text-muted", "text-faint",
                  "border-default", "border-subtle"]:
            lines.append(f"  --{k}: {t[k]};")
        for role in ORDER:
            for suffix in ("", "-soft", "-strong"):
                k = f"workspace-{role}{suffix}"
                lines.append(f"  --{k}: {t[k]};")
        mat = spec.get("material", {})
        for k, v in (mat.get("dark" if dark else "light", {})).items():
            lines.append(f"  --{k}: {v};")
        lines.append("}")
        out.append("\n".join(lines))
    return "\n\n".join(out)


def audit(name, spec):
    """对比度体检。返回问题清单。"""
    problems = []
    for dark in (False, True):
        t = build(spec, dark)
        mode = "dark" if dark else "light"
        c = contrast(t["text-primary"], t["surface-content"])
        if c < 7.0: problems.append(f"{name}/{mode} 正文对比度 {c:.1f} < 7.0")
        c = contrast(t["text-muted"], t["surface-content"])
        if c < 4.5: problems.append(f"{name}/{mode} 次要文字 {c:.1f} < 4.5")
        for role in ORDER:
            col, soft = t[f"workspace-{role}"], t[f"workspace-{role}-soft"]
            c = contrast(col, t["surface-content"])
            if c < 4.5: problems.append(f"{name}/{mode} {role} 身份色 {c:.1f} < 4.5")
            c = contrast(col, soft)
            if c < 4.5: problems.append(f"{name}/{mode} {role} 色/软底 {c:.1f} < 4.5")
            on = "#17131f" if dark else "#ffffff"
            c = contrast(on, col)
            if c < 4.3: problems.append(f"{name}/{mode} {role} 强调底上的文字 {c:.1f} < 4.3")
    return problems
