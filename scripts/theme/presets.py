#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""预设规格：11 套主题，分四组。

每套只需给出「主导色相 + 表面色相 + 彩度性格 + 材质」，
其余全部由 generate_themes.py 的角色语法推导，保证结构一致。
"""

# 材质：不同家族的边界、圆角与阴影性格不同，这是"换主题真的换了感觉"的关键。
CRISP = {  # 光家族：清晰的细边 + 干净的浅阴影
    "light": {"radius-control": "5px", "radius-panel": "7px",
              "shadow-menu": "0 10px 26px rgb(16 22 38 / 13%)",
              "shadow-panel": "-12px 0 26px rgb(16 22 38 / 13%)",
              "shadow-composer": "0 5px 16px rgb(16 22 38 / 8%)"},
    "dark":  {"shadow-menu": "0 12px 30px rgb(0 0 0 / 40%)",
              "shadow-panel": "-12px 0 28px rgb(0 0 0 / 40%)",
              "shadow-composer": "0 8px 22px rgb(0 0 0 / 28%)"},
}
INK = {  # 纸家族：几乎没有阴影，靠墨线与纸面明度分层；圆角极小
    "light": {"radius-control": "2px", "radius-panel": "3px",
              "shadow-menu": "0 6px 18px rgb(48 40 26 / 14%)",
              "shadow-panel": "-8px 0 18px rgb(48 40 26 / 12%)",
              "shadow-composer": "0 1px 0 rgb(48 40 26 / 7%)"},
    "dark":  {"shadow-menu": "0 8px 22px rgb(0 0 0 / 44%)",
              "shadow-panel": "-8px 0 20px rgb(0 0 0 / 40%)",
              "shadow-composer": "0 1px 0 rgb(0 0 0 / 30%)"},
}
SOFT = {  # 色家族：更大的圆角与更弥散的阴影，边界更轻
    "light": {"radius-control": "8px", "radius-panel": "12px",
              "shadow-menu": "0 14px 36px rgb(28 24 46 / 15%)",
              "shadow-panel": "-14px 0 34px rgb(28 24 46 / 15%)",
              "shadow-composer": "0 8px 26px rgb(28 24 46 / 10%)"},
    "dark":  {"shadow-menu": "0 16px 40px rgb(0 0 0 / 46%)",
              "shadow-panel": "-14px 0 36px rgb(0 0 0 / 44%)",
              "shadow-composer": "0 10px 28px rgb(0 0 0 / 32%)"},
}
FLAT = {  # 无障碍：无阴影、直角、实线边界
    "light": {"radius-control": "3px", "radius-panel": "4px",
              "shadow-menu": "0 0 0 1px #000000", "shadow-panel": "0 0 0 1px #000000",
              "shadow-composer": "0 0 0 1px #000000"},
    "dark":  {"shadow-menu": "0 0 0 1px #ffffff", "shadow-panel": "0 0 0 1px #ffffff",
              "shadow-composer": "0 0 0 1px #ffffff"},
}
WORKSHOP = {  # 工坊：近直角、无阴影，靠细边框划结构
    "light": {"radius-control": "4px", "radius-panel": "5px",
              "shadow-menu": "0 6px 16px rgb(18 22 30 / 12%)",
              "shadow-panel": "-8px 0 16px rgb(18 22 30 / 10%)",
              "shadow-composer": "0 2px 6px rgb(18 22 30 / 7%)"},
    "dark":  {"shadow-menu": "0 8px 20px rgb(0 0 0 / 42%)",
              "shadow-panel": "-8px 0 18px rgb(0 0 0 / 38%)",
              "shadow-composer": "0 3px 8px rgb(0 0 0 / 26%)"},
}
PLAIN = {  # 素面：中等圆角、极淡阴影，靠色块分区
    "light": {"radius-control": "6px", "radius-panel": "8px",
              "shadow-menu": "0 8px 22px rgb(24 24 32 / 10%)",
              "shadow-panel": "-10px 0 22px rgb(24 24 32 / 9%)",
              "shadow-composer": "0 2px 8px rgb(24 24 32 / 6%)"},
    "dark":  {"shadow-menu": "0 10px 26px rgb(0 0 0 / 40%)",
              "shadow-panel": "-10px 0 24px rgb(0 0 0 / 36%)",
              "shadow-composer": "0 3px 10px rgb(0 0 0 / 24%)"},
}

# ---------------------------------------------------------------------------
# 家族维度：颜色之外真正决定「长什么样」的四组取值。
#
# 加这一层的原因：原先 11 套预设只在颜色上不同，结构性令牌只有 4 种取值 ——
# 「色·现代主调」那四套的圆角、阴影、边框粗细完全一致，surface-raised 全是
# #ffffff，差异只剩一层近白底色的色相偏移。再加颜色也只是换汤不换药。
# ---------------------------------------------------------------------------

# 密度：控件高度、内边距、行距必须一起变 —— 只改高度不改内边距会让控件被挤扁。
# 档位之间约 1.2 倍等比；等差在小尺寸端显跳、在大尺寸端不够。
DENSITY = {
    "compact": {
        "control-height-xs": "18px", "control-height-sm": "22px", "control-height-md": "26px",
        "control-height-lg": "28px", "control-height-xl": "30px", "control-height-2xl": "34px",
        "control-height-3xl": "38px",
        "control-padding-sm": "5px", "control-padding-md": "7px", "control-padding-lg": "8px",
        "control-padding-xl": "10px", "control-padding-2xl": "12px", "control-padding-3xl": "14px",
    },
    # comfortable 不给任何覆盖：tokens.css 的默认值就是这一档，保持单一来源。
    "comfortable": {},
    "loose": {
        "control-height-xs": "22px", "control-height-sm": "26px", "control-height-md": "32px",
        "control-height-lg": "36px", "control-height-xl": "40px", "control-height-2xl": "44px",
        "control-height-3xl": "48px",
        "control-padding-sm": "8px", "control-padding-md": "10px", "control-padding-lg": "13px",
        "control-padding-xl": "16px", "control-padding-2xl": "18px", "control-padding-3xl": "20px",
    },
}

# 表面策略：靠什么区分一块表面的边界。三者互斥。
#
# 都不满足 WCAG 1.4.11 的 3:1 —— 实测 21/22 组合的 border-default 只有 1.5–1.7:1，
# 填充底色也在同一量级。所以 filled 并不比 outlined 更不可访问，二者共享既有基线；
# 合规职责由「高对比」那一套承担。详见 temp/audit-border-contrast.mjs。
STRATEGY = {
    "outlined": {},  # tokens.css 默认即 outlined
    "elevated": {
        "surface-border": "0",
        "surface-border-soft": "0",
        "surface-shadow": "0 1px 2px rgb(0 0 0 / 6%), 0 3px 10px rgb(0 0 0 / 7%)",
    },
    "filled": {
        "surface-border": "0",
        "surface-border-soft": "0",
        "surface-shadow": "none",
        # 无边框无阴影时，表面必须靠色差可辨；raised 在多数亮色主题是纯白，
        # 与 app 底色几乎无差，所以 filled 单独给一层。
        "surface-fill": "var(--surface-hover)",
        "control-border": "0",
        "control-fill": "var(--surface-hover)",
    },
}

# 字重节奏：层次靠字重拉开多少。
WEIGHT = {
    "flat": {"weight-emphasis": "500", "weight-strong": "560", "weight-heading": "600"},
    "moderate": {},  # tokens.css 默认即 moderate
    "strong": {"weight-emphasis": "620", "weight-strong": "700", "weight-heading": "780"},
}

# 家族 = 一组维度取值 + 一套材质。颜色仍由各预设自己给。
FAMILIES = {
    "workshop": dict(density="compact", strategy="outlined", weight="flat", material=WORKSHOP),
    "paper": dict(density="loose", strategy="filled", weight="strong", material=INK),
    "card": dict(density="comfortable", strategy="elevated", weight="moderate", material=SOFT),
    "plain": dict(density="comfortable", strategy="filled", weight="flat", material=PLAIN),
    "access": dict(density="comfortable", strategy="outlined", weight="strong", material=FLAT),
}


# 纸家族用更低的明度落差（纸不会白到 100%）
PAPER_LIGHT_LADDER = (0.930, 0.968, 0.982, 0.905, 0.895, 0.862, 0.922)
PAPER_DARK_LADDER = (0.195, 0.250, 0.298, 0.228, 0.328, 0.386, 0.278)

PRESETS = {
    # ---- 工坊：紧凑 + 细边框 + 平字重。信息密度优先 ----
    "graphite": dict(family="workshop", anchor=252, surface_hue=258, surface_chroma=0.004,
                     role_chroma=0.128, rail=(0.222, 0.012, 258)),
    "dawn": dict(family="workshop", anchor=228, surface_hue=242, surface_chroma=0.013,
                 role_chroma=0.118, rail=(0.300, 0.048, 236)),

    # ---- 纸本：宽松 + 填充 + 强字重。阅读优先 ----
    "xuan": dict(family="paper", anchor=26, surface_hue=77, surface_chroma=0.019,
                 role_chroma=0.126, rail=(0.228, 0.016, 52), border_l=0.808,
                 dark_surface_chroma=0.52,
                 light_ladder=PAPER_LIGHT_LADDER, dark_ladder=PAPER_DARK_LADDER),
    "cyanotype": dict(family="paper", anchor=256, surface_hue=250, surface_chroma=0.024,
                      role_chroma=0.120, rail=(0.248, 0.078, 258), spread=0.72,
                      border_l=0.815, light_ladder=PAPER_LIGHT_LADDER, dark_ladder=PAPER_DARK_LADDER),
    "paper": dict(family="paper", anchor=196, surface_hue=80, surface_chroma=0.027,
                  role_chroma=0.124, rail=(0.262, 0.028, 66), spread=1.14, dark_surface_chroma=0.5,
                  border_l=0.818, light_ladder=PAPER_LIGHT_LADDER, dark_ladder=PAPER_DARK_LADDER),

    # ---- 卡片：舒适 + 投影 + 中字重。层次优先 ----
    "mnemora": dict(family="card", anchor=296, surface_hue=286, surface_chroma=0.015,
                    role_chroma=0.126, rail=(0.262, 0.076, 292)),
    "ocean": dict(family="card", anchor=212, surface_hue=226, surface_chroma=0.016,
                  role_chroma=0.122, rail=(0.252, 0.066, 232)),
    "lamp": dict(family="card", anchor=58, surface_hue=68, surface_chroma=0.017,
                 role_chroma=0.108, rail=(0.232, 0.026, 54), dark_surface_chroma=0.62,
                 light_ladder=(0.948, 0.982, 0.996, 0.922, 0.912, 0.878, 0.938)),

    # ---- 素面：舒适 + 填充 + 平字重。安静优先 ----
    "forest": dict(family="plain", anchor=162, surface_hue=147, surface_chroma=0.016,
                   role_chroma=0.116, rail=(0.272, 0.056, 158)),
    "rose": dict(family="plain", anchor=12, surface_hue=352, surface_chroma=0.013,
                 role_chroma=0.128, rail=(0.272, 0.070, 356)),

    # ---- 无障碍：舒适 + 细边框 + 强字重。可达性优先 ----
    "highContrast": dict(family="access", anchor=250, surface_hue=250, surface_chroma=0.0,
                         role_chroma=0.150, rail=(0.055, 0.0, 250),
                         role_l_shift=-0.075, border_l=0.660,
                         light_ladder=(0.968, 1.000, 1.000, 0.940, 0.928, 0.882, 0.952)),
}

FAMILY_ORDER = ["workshop", "paper", "card", "plain", "access"]
PRESET_ORDER = ["graphite", "dawn",
                "xuan", "cyanotype", "paper",
                "mnemora", "ocean", "lamp",
                "forest", "rose",
                "highContrast"]

# 每套预设最终展开的维度取值 —— 展开后 `group` 字段仍供设置页分组使用。
for _name, _spec in PRESETS.items():
    _family = FAMILIES[_spec["family"]]
    _spec.setdefault("material", _family["material"])
    _spec["group"] = _spec["family"]
    _spec["density"] = _family["density"]
    _spec["strategy"] = _family["strategy"]
    _spec["weight"] = _family["weight"]

GROUP_ORDER = FAMILY_ORDER
