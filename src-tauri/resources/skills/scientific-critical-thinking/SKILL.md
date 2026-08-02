---
id: scientific-critical-thinking
name: 科学证据分析
description: 系统评估科学主张、研究设计、统计推断、偏差、混杂因素和证据质量；适合论文评价与研究结论核查。
version: 1.0.0
license: MIT
compatibility: 分析方法本身无需联网；有 PDF 附件时可结合按页读取，否则只分析当前上下文已有材料。
triggers:
  - /evidence
  - /critical
argument-hint: "<需要评价的主张、方法或证据>"
recommended-tools:
  - read_pdf_pages
metadata:
  mnemora:
    default-enabled: false
    supported-modes: [chat, work]
    risk: low
    resource-cost: medium
    source-repository: https://github.com/K-Dense-AI/scientific-agent-skills
    source-path: skills/scientific-critical-thinking/SKILL.md
    source-revision: 831d49eb77eed3c792be2970921b46764012ef00
    attribution: "Scientific Critical Thinking by K-Dense Inc., licensed under MIT."
    adapted: true
    adaptation-notes: 保留方法学、偏差、统计和证据分级框架；移除 OpenRouter 图示生成、文件写入及外部参考文件依赖。
---

# 科学证据分析

评价的目标是确定“哪些结论被证据支持、支持到什么程度”，不是简单复述，也不是为了挑错。

## 评价框架

1. **问题与设计**：研究问题是否清楚；实验、观察或准实验设计能否支持作者的推断。
2. **内部效度**：随机化、分配隐藏、盲法、对照、失访和混杂控制是否充分。
3. **外部效度**：样本、场景和干预是否能推广到目标人群与真实环境。
4. **测量质量**：变量定义、测量工具、代理指标和结果评价是否可靠。
5. **统计推断**：样本量、检验选择、模型假设、多重比较、缺失数据、效应量和置信区间是否合理。
6. **偏差与报告**：选择偏差、测量偏差、发表偏差、结果切换、P-hacking 和利益冲突。
7. **证据等级**：结合设计类型、偏差风险、一致性、直接性、精确性和可复现性给出有条件的置信判断。

## 输出结构

- 核心判断
- 研究优势
- 关键问题，按“严重、重要、次要”排序
- 证据能够支持的结论
- 证据不能支持或仍不确定的结论
- 需要补充的信息或验证步骤

始终区分数据与解释、相关与因果、统计显著与实际重要性。材料不足时使用条件判断，不补写不存在的方法或结果。
