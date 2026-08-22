---
id: hatch-pet
name: 宠物孵化与安装
description: 设计、检查和打包 Mnemora/Codex 兼容桌面宠物；适用于创建宠物概念、规划 8×9 动画状态、核对 Sprite Atlas，或准备可直接在设置中安装的宠物 ZIP。
version: 1.0.0
license: Apache-2.0
compatibility: 纯提示词与资源规范技能；若当前模型没有图像生成工具，应输出制作规格而不是声称已经生成图片。
triggers: [/hatch-pet, /pet-pack]
argument-hint: "<宠物形象、风格、品牌线索或现有角色图>"
metadata:
  mnemora:
    default-enabled: false
    supported-modes: [chat, notes]
    risk: low
    resource-cost: medium
    attribution: "Adapted from the installed OpenAI curated hatch-pet skill and Codex pet atlas contract."
    source-repository: https://github.com/openai/skills
    source-path: skills/.curated/hatch-pet/SKILL.md
    source-revision: 49f948faa9258a0c61caceaf225e179651397431
    adapted: true
    adaptation-notes: 保留 8×9 Atlas、状态行、透明背景、视觉 QA 和纯资源包边界；安装目标改为 Mnemora 设置页。
---
# 宠物孵化与安装

目标是产出一个只包含资源、可审计、可直接安装的宠物包。不得把插件脚本、可执行文件、网络地址或模型权限放进宠物包。

## 能力边界

- 当前环境有图像生成能力时，可以生成角色基准图和状态行。
- 当前环境没有图像生成能力时，输出完整的视觉规格、逐行姿态要求和验收清单；不要声称图片已经生成。
- Mnemora 的“设置 → 桌面宠物 → 安装宠物包”可直接安装最终 ZIP，不要求用户安装 Codex。

## 固定 Atlas 合同

- 图片：`spritesheet.webp`，透明背景；
- 尺寸：1536×1872；
- 网格：8 列 × 9 行；
- 单帧：192×208；
- 未使用单元格必须完全透明；
- 不包含文字、网格线、场景背景、外部阴影或跨单元格内容。

| 行 | 状态 | 使用帧 | 目的 |
| --- | --- | --- | --- |
| 0 | idle | 0–5 | 低干扰呼吸、眨眼 |
| 1 | running-right | 0–7 | 向右移动 |
| 2 | running-left | 0–7 | 向左移动 |
| 3 | waving | 0–3 | 问候或吸引注意 |
| 4 | jumping | 0–4 | 起跳、峰值、落地 |
| 5 | failed | 0–7 | 失败或沮丧反馈 |
| 6 | waiting | 0–5 | 等待用户确认或输入 |
| 7 | running | 0–5 | 正在思考、读取或执行任务，不是跑步 |
| 8 | review | 0–5 | 审阅、检查和聚焦 |

## 包结构

```text
<pet-id>/
├── pet.json
└── spritesheet.webp
```

```json
{
  "id": "lowercase-pet-id",
  "displayName": "Pet Name",
  "description": "One short sentence.",
  "spritesheetPath": "spritesheet.webp",
  "kind": "custom"
}
```

最终 ZIP 可以在根目录直接包含两个文件，也可以只包一层 `<pet-id>/` 目录。不要加入脚本、README、缩略图或其他文件。

## 视觉 QA

1. 九行保持相同角色身份、脸、比例、材质、配色和道具。
2. 每一帧完整落在自己的 192×208 单元格中。
3. idle 足够安静；waiting 与 idle 可区分；running 表达工作状态而非奔跑。
4. 透明像素不保留明显色边；无断裂像素、裁肢或相邻帧重叠。
5. 在 72px 和 176px 两种显示尺寸下仍能读出动作。
6. 安装前核对 `pet.json` ID、WebP 尺寸和文件大小。
