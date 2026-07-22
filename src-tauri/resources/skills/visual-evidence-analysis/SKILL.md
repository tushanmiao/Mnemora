---
id: visual-evidence-analysis
name: 图片证据分析
description: 对用户提供的图片、截图、图表或扫描页面进行可核查的观察、文字识别、结构分析和不确定性说明，避免把推测写成图中事实。
version: 1.0.0
license: MIT
compatibility: 依赖所选模型实际支持并接收到图片；不执行独立 OCR、图片编辑、区域裁切或像素级测量。
triggers:
  - /image
  - /vision
argument-hint: "<需要识别、比较或验证的内容>"
metadata:
  mnemora:
    source-repository: https://github.com/github/awesome-copilot
    source-path: skills/eyeball/SKILL.md
    source-revision: 786bdcfc65b669faee10803db460a7218858ad21
    attribution: "Eyeball skill from GitHub's awesome-copilot repository, licensed under MIT."
    adapted: true
    adaptation-notes: 保留视觉证据可验证和事实紧邻来源的原则；移除 Word 输出、截图标注、Python、Playwright 和外部工具依赖，改为直接分析会话图片。
---

# 图片证据分析

如果当前模型没有收到图片、图片不可读或分辨率不足，必须明确说明，不能根据文件名和用户描述假装看见内容。

## 分析步骤

1. 先概览图片类型、主要区域、布局和用户真正要确认的问题。
2. 按从左到右、从上到下或按明确区域逐项观察，避免漏掉边角信息。
3. 将结果分为：**直接可见**、**合理推断**、**无法确认**。
4. 识别文字时保持原有数字、单位、大小写和符号；模糊字符使用方括号或问号标记。
5. 分析图表时说明标题、坐标轴、单位、图例、趋势、异常点和数据是否足以支持结论。
6. 比较多张图片时使用一致维度，并指出视角、缩放、时间或裁剪差异可能造成的误判。

## 输出规则

- 先直接回答用户的问题，再给观察依据。
- 不把身份、地点、因果关系、情绪或精确数值从模糊视觉线索中过度推断。
- 不声称完成了 OCR、图像增强或像素测量；只能说明模型当前能够读取到的内容。
- 涉及文档页时，保留页眉、页脚、表格结构和可能影响理解的版面关系。
