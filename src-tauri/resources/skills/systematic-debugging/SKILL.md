---
id: systematic-debugging
name: 系统化调试
description: 面向错误、测试失败、白屏、性能异常和行为回归，先建立可复现证据与根因链，再提出最小修复和验证方案。
version: 1.0.0
license: MIT
compatibility: 不要求特定工具；只能基于当前对话、用户提供的日志、代码和附件进行诊断，不能假装执行了未提供的命令。
triggers:
  - /debug
  - /diagnose
argument-hint: "<现象、复现步骤、日志或最近变更>"
recommended-tools:
  - read_attachment_text
metadata:
  mnemora:
    source-repository: https://github.com/obra/superpowers
    source-path: skills/systematic-debugging/SKILL.md
    source-revision: d884ae04edebef577e82ff7c4e143debd0bbec99
    attribution: "Systematic Debugging by Jesse Vincent, licensed under MIT."
    adapted: true
    adaptation-notes: 保留先调查根因、建立假设、最小实验和回归验证的核心方法；移除 Codex/Claude 工作区命令和强制流程措辞。
---

# 系统化调试

不要从症状直接跳到修复。先确认问题在哪一层发生，以及哪条证据能够区分不同假设。

## 调试流程

1. **固定现象**：记录期望行为、实际行为、错误原文、环境和最短复现步骤。
2. **缩小范围**：判断问题属于输入、状态、前端渲染、IPC、Rust 服务、供应商协议、网络还是持久化。
3. **查看变化**：检查最近变更、配置差异、依赖版本和只在特定数据出现的条件。
4. **建立假设**：为每个候选根因写出支持证据、反证和可区分它们的最小实验。
5. **验证根因**：先用日志、断点、最小样例或单一变量实验确认，再修改代码。
6. **最小修复**：修复产生问题的原因，并保持行为边界清楚。
7. **回归验证**：复现原问题，运行相关测试，再检查相邻路径和资源释放。

输出时将“已经证实”“最可能”“尚需验证”分开。信息不足时优先索取最有区分度的一项证据，而不是罗列大量泛化建议。
