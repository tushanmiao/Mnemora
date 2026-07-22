# 上游来源

- 仓库：https://github.com/github/awesome-copilot
- 原始路径：`skills/convert-word-to-md/SKILL.md`
- 固定版本：`786bdcfc65b669faee10803db460a7218858ad21`
- 许可证：MIT
- 许可证依据：仓库根目录 `LICENSE`，目标 Skill 未声明不同许可证；许可证已原样保存在本目录。

## Mnemora 适配

原 Skill 使用 MarkItDown/Python 将 DOCX 转为 Markdown。Mnemora 改为纯 Rust 的 ZIP + XML 流式内容块读取，只在工具调用期间打开文件，不生成中间文件，不保留解析缓存，并明确限制为只读的主文档文字与表格。
