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

# 纸家族用更低的明度落差（纸不会白到 100%）
PAPER_LIGHT_LADDER = (0.930, 0.968, 0.982, 0.905, 0.895, 0.862, 0.922)
PAPER_DARK_LADDER = (0.195, 0.250, 0.298, 0.228, 0.328, 0.386, 0.278)

PRESETS = {
    # ---- 光：一天中的光线质地 ----
    "dawn": dict(group="light", anchor=228, surface_hue=242, surface_chroma=0.013,
                 role_chroma=0.118, rail=(0.300, 0.048, 236), material=CRISP),
    "lamp": dict(group="light", anchor=58, surface_hue=68, surface_chroma=0.017,
                 role_chroma=0.108, rail=(0.232, 0.026, 54), material=CRISP, dark_surface_chroma=0.62,
                 light_ladder=(0.948, 0.982, 0.996, 0.922, 0.912, 0.878, 0.938)),
    "graphite": dict(group="light", anchor=252, surface_hue=258, surface_chroma=0.004,
                     role_chroma=0.128, rail=(0.222, 0.012, 258), material=CRISP),

    # ---- 纸：真实纸张与颜料 ----
    "xuan": dict(group="paper", anchor=26, surface_hue=77, surface_chroma=0.019,
                 role_chroma=0.126, rail=(0.228, 0.016, 52), material=INK, border_l=0.808,
                 dark_surface_chroma=0.52,
                 light_ladder=PAPER_LIGHT_LADDER, dark_ladder=PAPER_DARK_LADDER),
    "cyanotype": dict(group="paper", anchor=256, surface_hue=250, surface_chroma=0.024,
                      role_chroma=0.120, rail=(0.248, 0.078, 258), material=INK, spread=0.72,
                      border_l=0.815, light_ladder=PAPER_LIGHT_LADDER, dark_ladder=PAPER_DARK_LADDER),
    "paper": dict(group="paper", anchor=196, surface_hue=80, surface_chroma=0.027,
                  role_chroma=0.124, rail=(0.262, 0.028, 66), material=INK, spread=1.14, dark_surface_chroma=0.5,
                  border_l=0.818, light_ladder=PAPER_LIGHT_LADDER, dark_ladder=PAPER_DARK_LADDER),

    # ---- 色：饱满的现代主调 ----
    "mnemora": dict(group="color", anchor=296, surface_hue=286, surface_chroma=0.015,
                    role_chroma=0.126, rail=(0.262, 0.076, 292), material=SOFT),
    "forest": dict(group="color", anchor=162, surface_hue=147, surface_chroma=0.016,
                   role_chroma=0.116, rail=(0.272, 0.056, 158), material=SOFT),
    "ocean": dict(group="color", anchor=212, surface_hue=226, surface_chroma=0.016,
                  role_chroma=0.122, rail=(0.252, 0.066, 232), material=SOFT),
    "rose": dict(group="color", anchor=12, surface_hue=352, surface_chroma=0.013,
                 role_chroma=0.128, rail=(0.272, 0.070, 356), material=SOFT),

    # ---- 无障碍 ----
    "highContrast": dict(group="access", anchor=250, surface_hue=250, surface_chroma=0.0,
                         role_chroma=0.150, rail=(0.055, 0.0, 250), material=FLAT,
                         role_l_shift=-0.075, border_l=0.660,
                         light_ladder=(0.968, 1.000, 1.000, 0.940, 0.928, 0.882, 0.952)),
}

GROUP_ORDER = ["light", "paper", "color", "access"]
PRESET_ORDER = ["dawn", "lamp", "graphite", "xuan", "cyanotype", "paper",
                "mnemora", "forest", "ocean", "rose", "highContrast"]
