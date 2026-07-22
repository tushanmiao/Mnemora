---
id: paper-research
name: 论文研究
description: 对当前会话中的一篇或多篇论文进行问题、方法、证据、结论、限制和文献间差异分析，并保留 PDF 页码来源。
version: 1.0.0
license: MIT
compatibility: 当前适配版只研究用户已附加的 PDF，不提供 Semantic Scholar、arXiv 搜索或联网下载。
triggers:
  - /paper
  - /papers
argument-hint: "<研究问题、论文范围或比较维度>"
recommended-tools:
  - read_pdf_pages
required-tools:
  - read_pdf_pages
metadata:
  mnemora:
    source-repository: https://github.com/xwmxcz/papers-skill
    source-path: skills/papers-research/SKILL.md
    source-revision: a64c2eda2c9fc182c96e1409cde267b262dbebde
    attribution: "Papers Research by xwmxcz, licensed under MIT."
    adapted: true
    adaptation-notes: 将原有论文搜索、下载和 Python CLI 流程改为 Mnemora 已有的会话 PDF 按页读取；保留深读、比较和证据追踪方法。
---

# 论文研究

使用当前会话中实际存在的论文完成研究。当前版本不能在线检索文献，也不能把摘要或标题当作全文证据。

## 单篇深读

1. 识别研究问题、背景缺口和作者主张。
2. 读取方法、数据、样本、基线、指标和统计分析所在页面。
3. 将主要结论映射到直接证据，判断结论强度是否超过研究设计能够支持的范围。
4. 检查限制、利益冲突、数据可用性和作者未排除的替代解释。
5. 输出“问题、方法、关键结果、证据强度、限制、待确认问题”。

## 多篇比较

- 先分别建立“论文 - 主张 - 方法 - 证据 - 限制”记录，再进行横向综合。
- 区分一致结论、表面一致但定义不同、真实冲突和材料不足。
- 不以论文数量或引用次数代替证据质量。
- 每个关键判断保留对应论文的 `[PDF:附件ID#page=页码]` 引用。

用户要求“最新研究”或“完整检索”时，明确说明当前 Skill 只覆盖已附加材料。
