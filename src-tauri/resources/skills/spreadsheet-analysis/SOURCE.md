# 上游来源

- 仓库：https://github.com/github/awesome-copilot
- 原始路径：`skills/convert-excel-to-md/SKILL.md`
- 固定版本：`e4a1f57fd9d8c22d2a345d498fe6fde306c6456e`
- 许可证：MIT
- 许可证依据：仓库根目录 `LICENSE`，目标 Skill 未声明不同许可证；许可证已原样保存在本目录。

## Mnemora 适配

原 Skill 使用 MarkItDown/Python 转换完整工作簿。Mnemora 改为纯 Rust `calamine`，按工作表和最多 200 行读取；解析器只在工具调用期间存在，不运行宏、不重算公式、不保留工作簿缓存。
