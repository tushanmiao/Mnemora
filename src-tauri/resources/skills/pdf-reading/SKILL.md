---
id: pdf-reading
name: PDF 阅读
description: 按需读取当前会话 PDF 的指定页面，提取可核查内容并保留页码引用；适合报告、论文、合同和普通文档阅读。
version: 1.0.0
license: Apache-2.0
compatibility: 需要当前会话中存在带文本层的 PDF；扫描件 OCR、PDF 编辑、合并和创建尚未实现。
triggers:
  - /pdf
  - /pdf-read
argument-hint: "<阅读范围、页码或重点问题>"
recommended-tools:
  - read_pdf_pages
required-tools:
  - read_pdf_pages
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work]
    risk: low
    resource-cost: high
    source-repository: https://github.com/openai/skills
    source-path: skills/.curated/pdf/SKILL.md
    source-revision: 49f948faa9258a0c61caceaf225e179651397431
    attribution: "PDF Skill by OpenAI, licensed under Apache-2.0."
    adapted: true
    adaptation-notes: 仅保留与 Mnemora 当前按页文本读取能力相符的工作流；移除 Python、Poppler、PDF 创建和视觉渲染依赖。
---

# PDF 阅读

只分析 `read_pdf_pages` 实际返回的页面。不要根据文件名、目录、缩略图或未读取页面推测内容。

## 工作流程

1. 明确用户的问题、附件和期望覆盖的页码范围。
2. 范围不明确时，从目录、摘要、引言或用户指定页面开始，小批量读取。
3. 对关键结论继续读取其前后页面，避免脱离上下文引用。
4. 区分原文事实、作者观点、你的归纳和仍需验证的内容。
5. 每项可核查结论紧邻保留工具返回的 `[PDF:附件ID#page=页码]` 标识。

## 输出规则

- 先回答用户的问题，再补充证据和页码。
- 直接引语保持简短；无法确认原句时使用转述。
- 页面无法提取、页码不存在或证据不足时明确说明。
- 不声称已经执行 OCR、修改 PDF、写入批注或保存外部文件。
