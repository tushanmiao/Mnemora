---
id: docx-reading
name: Word 文档阅读
description: 按内容块读取当前会话 DOCX 的正文与表格，进行摘要、结构梳理、信息提取和对比，并保留可追踪的块编号。
version: 1.0.0
license: MIT
compatibility: 只读取 10 MB 以内 DOCX 的主文档正文和表格；不支持旧版 DOC、页眉页脚、批注、图片 OCR、排版还原、编辑或导出。
triggers:
  - /docx
  - /word
argument-hint: "<阅读范围、提取字段或分析问题>"
recommended-tools:
  - read_docx_blocks
required-tools:
  - read_docx_blocks
metadata:
  mnemora:
    default-enabled: false
    supported-modes: [chat, work]
    risk: low
    resource-cost: medium
    source-repository: https://github.com/github/awesome-copilot
    source-path: skills/convert-word-to-md/SKILL.md
    source-revision: 786bdcfc65b669faee10803db460a7218858ad21
    attribution: "Convert Word to Markdown skill from GitHub's awesome-copilot repository, licensed under MIT."
    adapted: true
    adaptation-notes: 将原版 MarkItDown/Python 转换工作流改为 Mnemora 纯 Rust、有界的 DOCX 内容块读取；不创建中间 Markdown 文件。
---

# Word 文档阅读

只分析 `read_docx_blocks` 实际返回的内容。块编号表示文档顺序，不等同于 Word 页码。

## 工作流程

1. 先根据用户问题确定需要读取的内容范围；范围未知时从前 20 到 50 个块开始。
2. 识别标题、段落、列表和工具返回的表格行，建立文档结构。
3. 需要精确提取时，逐项保留 `[DOCX:附件ID#block=编号]` 来源。
4. 对合同、报告或方案，区分事实、义务、条件、例外、日期、金额和待确认内容。
5. 对多个文档进行比较前，先分别形成记录，再按相同维度对齐。

## 边界

- 不根据文件名或未读取内容补写结论。
- 不把块编号称为页码。
- 不声称看到了图片、页眉页脚、批注或排版效果。
- 当前工具只读，不会修改或保存 Word 文件。
