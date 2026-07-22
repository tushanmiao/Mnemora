# Mnemora

轻量、流畅、可扩展的桌面 AI 对话与阅读工作台。

Mnemora 使用 Tauri 2 + Rust + React + TypeScript 构建。它把模型请求、凭据、文件解析和安全边界放在 Rust 侧，把交互和渲染放在 React 侧，目标是在保持多模型能力的同时控制常驻内存和长对话卡顿。

> 当前版本是 Windows x64 测试版本。项目仍在快速迭代中，PDF 文献库、PDF 笔记/批注和 Office 文件写入能力尚未完成。

## 当前版本

- 版本：0.1.0
- 平台：Windows x64
- 最新测试发布：[GitHub Releases](https://github.com/tushanmiao/Mnemora/releases)
- 默认安装包：NSIS
- 备用安装包：MSI

## 主要能力

### 多模型与中转站

- OpenAI Chat Completions
- OpenAI Responses
- Anthropic Messages
- Gemini GenerateContent
- 自定义 Provider 和中转站
- API Model 与 Display Name 分离
- 手动测试连接和手动获取模型，不在启动时自动请求供应商
- API Key 使用系统凭据存储，不写入普通设置 JSON

### Chat 与 Agent

- 非流式和 SSE 流式对话
- 独立显示思考内容
- 停止生成、有限重试、编辑、重新生成和删除消息
- AI 权限模式与敏感工具审批
- 受限制的 Agent Loop
- /help、/new、/clear、/compact、/model、/settings、/skills、/memory、/attach 等本地命令
- 上下文用量估算，接近 90% 时自动压缩上下文

当前 Agent 只使用固定且有边界的工具，能够读取文本、图片、PDF、DOCX、XLSX 和受控记忆；它没有任意 Shell、任意进程启动或任意桌面控制权限。

### Skill 与记忆

- 内置 PDF 阅读、论文研究、科学证据分析、Markdown 笔记、知识整理、代码分析、图片分析、Word 和 Excel 分析 Skill
- Skill 带有来源说明和许可证文件
- 用户可以导入、启用、禁用、卸载和恢复 Skill
- Skill 正文按需加载，每次对话限制激活数量
- L1/L2 Markdown 记忆
- 记忆读取和写入权限可分别配置
- 拒绝将 API Key、密码和疑似提示注入内容写入长期记忆

### 流畅性与低占用

- 流式增量先进入独立 store，约 30 FPS 合并发布
- 只有当前 Assistant 消息订阅高频流状态
- Markdown 按块切分，已完成块保持稳定，只解析尾部块
- 会话索引和会话详情分离，按需加载完整会话
- 用量和请求调试均有大小上限
- 主窗口关闭后销毁 WebView，只保留 Rust 和托盘
- 开机自启时不创建主 WebView
- HTML 预览窗口按需创建，关闭后销毁

### Markdown、HTML 与附件

- GitHub 风格 Markdown 和表格
- 安全 HTML 片段白名单
- HTML 代码块独立预览
- 预览内容使用清洗、CSP 和无权限 sandbox iframe
- 支持文本、图片、PDF、DOCX、XLSX 附件
- 附件按会话隔离，解析任务完成后释放临时资源

### 用量与调试

- 按供应商、模型和操作统计 Token
- 输入、输出、缓存、思考 Token
- ProviderReported、GatewayNormalized、Estimated、Missing 来源区分
- 成本、价格快照、首字时间和输出速度
- Agent 每轮模型调用单独记录
- 请求调试默认关闭，只保留最近 30 条脱敏记录

## 安装

1. 打开 [Mnemora Releases](https://github.com/tushanmiao/Mnemora/releases)。
2. 下载 Mnemora_0.1.0_x64-setup.exe。
3. 运行安装程序并按向导完成安装。
4. 首次启动后，在“设置 → 模型服务”中添加 Provider、API Model 和 API Key。
5. 回到聊天页面选择模型，开始对话。

当前安装包未进行代码签名。Windows 可能显示“未知发布者”或 SmartScreen 提示，这是测试版本的正常现象。

## 本地数据与隐私

Mnemora 的数据默认保存在系统应用数据目录：

    app_config_dir/
      app-settings.json
      model-settings.json

    app_data_dir/
      conversations/
      memory/global/
      skills/
      usage/

API Key 使用 Windows Credential Manager 等系统凭据存储。设置导出功能可以主动把 Provider API Key 和可选记忆写入 JSON 备份，因此导出文件必须按密码备份保管，不要提交到 Git 或公开分享。

Mnemora 不会在启动时自动测试中转站，也不会自动轮询供应商。请求调试默认关闭，开启后也只保留有界、脱敏的内存记录。

## 技术栈

| 层次 | 技术 |
| --- | --- |
| 桌面壳 | Tauri 2 |
| 前端 | React 19、TypeScript 5.8 |
| 构建 | Vite 7 |
| 后端 | Rust 2021、Tokio |
| 网络 | reqwest + rustls |
| 数据 | serde、serde_json、JSON 文件 |
| 凭据 | keyring、zeroize |
| Markdown | react-markdown、remark-gfm、rehype-raw、rehype-sanitize |
| 文档解析 | lopdf、quick-xml、calamine、image、zip |

## 项目结构

    src/
      bootstrap/       启动诊断和错误边界
      features/chat/   Chat、流式状态、Markdown 和附件
      features/settings/ 基础、模型、Skill、记忆、用量和调试设置
      features/skills/ Skill 管理界面
      features/html-preview/ HTML 预览外壳
      features/conversations/ 会话侧栏和缓存
      types/           前端类型合同

    src-tauri/src/
      ai/              供应商无关模型层和协议适配器
      chat/            Chat 服务、会话、附件和 Agent
      commands/        Tauri IPC 命令
      memory/          L1/L2 记忆
      settings/        设置和系统凭据
      skills/          Skill 解析、安装和仓库
      usage/           用量记录和统计
      window_lifecycle.rs

## 开发环境

需要安装：

- Node.js
- Rust 工具链
- Tauri 2 所需的 Windows WebView2 和构建工具

安装依赖后：

    npm install
    npm run tauri dev

常用验证命令：

    npm test
    npm run build
    cd src-tauri
    cargo check --lib
    cargo test --lib

发布构建：

    npm run tauri build

产物位于：

    src-tauri/target/release/mnemora.exe
    src-tauri/target/release/bundle/nsis/
    src-tauri/target/release/bundle/msi/

## 当前限制与路线

### 正在规划或开发

- Zotero 类文献库和 PDF 阅读器
- PDF 页码定位、笔记、批注、标签和集合
- PDF 内容与会话、Skill、记忆的关联
- OfficeCLI 受控工具
- 对话内容的 Markdown、HTML、PDF 等自定义导出
- 更完整的模型能力探测和协议兼容测试

### 明确不提供

- 任意 Shell 命令执行
- 任意桌面 GUI 自动化
- 将 Skill 当作可执行插件运行
- 把任意 OfficeCLI 命令字符串直接交给模型

OfficeCLI 未来如果接入，也会通过固定工具、用户审批、安全副本、备份和结果校验运行；它不等于控制桌面版 Word 或 Excel。

## 第三方 Skill

内置 Skill 的每个目录都包含 SOURCE.md 和 LICENSE.txt。使用、修改或再分发这些内容时，请同时遵守对应来源项目的许可证和署名要求。第三方来源汇总见：

[src-tauri/resources/skills/THIRD_PARTY_NOTICES.md](src-tauri/resources/skills/THIRD_PARTY_NOTICES.md)

## 项目文档

- [项目计划](md/plan/)
- [参考研究](md/reference/)
- [项目总结](md/Summary/)
- [Git 命令记录](md/git_order/)

md 目录用于学习和设计记录，按项目约定不提交到 Git。

## 许可证说明

当前仓库根目录尚未提供统一的 LICENSE 文件。除各第三方 Skill 自带的许可证外，不应默认将 Mnemora 源码视为可自由再分发内容。

## 反馈与贡献

问题反馈和功能建议请提交到 [GitHub Issues](https://github.com/tushanmiao/Mnemora/issues)。提交问题时，请说明：

- Windows 版本和安装包类型
- Mnemora 版本
- 是否使用官方 Provider 或中转站
- 复现步骤和错误信息
- 是否可以提供脱敏后的启动诊断
