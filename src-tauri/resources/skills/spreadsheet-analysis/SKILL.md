---
id: spreadsheet-analysis
name: Excel 表格分析
description: 按工作表和行范围读取当前会话 XLSX，进行字段理解、数据核对、汇总、异常识别和跨表比较，并保留工作表与行号来源。
version: 1.0.0
license: MIT
compatibility: 只读取 10 MB 以内 XLSX 的单元格值；不支持旧版 XLS、宏执行、公式重算、图表/图片识别、样式还原、编辑或导出。
triggers:
  - /xlsx
  - /excel
argument-hint: "<工作表、行范围或分析目标>"
recommended-tools:
  - read_xlsx_rows
required-tools:
  - read_xlsx_rows
metadata:
  mnemora:
    source-repository: https://github.com/github/awesome-copilot
    source-path: skills/convert-excel-to-md/SKILL.md
    source-revision: 786bdcfc65b669faee10803db460a7218858ad21
    attribution: "Convert Excel to Markdown skill from GitHub's awesome-copilot repository, licensed under MIT."
    adapted: true
    adaptation-notes: 将原版 MarkItDown/Python 转换流程改为 Mnemora 纯 Rust、按工作表和行范围读取；不生成中间文件，也不运行宏或公式。
---

# Excel 表格分析

使用 `read_xlsx_rows` 先取得工作表目录，再按需读取小范围行。不要一次请求整本工作簿。

## 工作流程

1. 未指定工作表时先查看工具返回的工作表名称和首批行，确认表头与数据含义。
2. 识别字段、单位、日期范围、主键、空值、合计行和可能的重复记录。
3. 只在读取到的数据范围内进行计算；说明筛选条件、分母、单位和舍入方式。
4. 发现异常时给出具体工作表与行号，例如 `[XLSX:附件ID#sheet=数据#row=12]`。
5. 跨表比较时先确认字段语义一致，不能只因列名相同就直接合并。

## 输出规则

- 区分原始单元格值、你的计算结果和解释。
- 对空值、文本数字、日期格式和缓存公式结果保持谨慎。
- 不声称已经执行宏、重算公式、读取图表或修改工作簿。
- 数据不足时指出还需要读取的工作表和行范围。
