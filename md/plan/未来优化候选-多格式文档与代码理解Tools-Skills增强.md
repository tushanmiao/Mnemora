# Mnemora 未来优化候选：多格式文档、图片与代码理解 Tools / Skills 增强

> 文档性质：未来优化候选，不占用正式 Plan 序号。
>
> 当前状态：仅完成方向调研与候选架构分析，尚未形成正式实施决策，也不代表已经开始接入相关依赖。
>
> 后续处理：当该方向进入实际开发阶段时，应重新进行版本、许可证、Windows 构建、安装体积、内存和解析质量审计，再形成带正式序号的详细 Plan。
>
> 调研时间：2026 年 8 月 13 日。第三方项目状态可能变化，实施前必须以当时的仓库、Release、许可证和依赖树为准。

---

## 一、候选方向的核心结论

Mnemora 当前的模型理解能力已经较强，PDF、DOCX、XLSX、普通文本和代码文件也已经具备基础读取能力。下一阶段真正需要补齐的，不是继续用提示词描述“应该怎样阅读文件”，而是增强能够真实取得、定位和验证文件内容的 Tool。

三类能力的边界如下：

| 层级 | 主要职责 | 不能替代的部分 |
| --- | --- | --- |
| Model（模型） | 理解、归纳、推理、解释和生成 | 不能凭空获得尚未解析的文件内容 |
| Tool（工具） | 读取、渲染、OCR、定位、检索和结构化提取 | 不能自行决定高质量分析方法 |
| Skill（技能） | 规定阅读流程、问题框定、证据标准和讲解方法 | 不能通过提示词伪造 Tool 的执行结果 |

因此，未来应采用：

> **轻量 Rust 核心作为默认路径，视觉模型作为按需回退，OCR 和复杂文档引擎作为可选、可销毁的独立扩展；Skill 只编排真实存在的 Tool。**

不建议挑选一个“大而全”的项目，直接替换 Mnemora 当前全部解析实现。这样的接入方式通常会同时带来安装体积、冷启动、内存、依赖、许可证和故障隔离问题。

---

## 二、当前基础与主要能力缺口

### 2.1 当前已有基础

当前已经具备或已经规划使用的基础能力包括：

- `read_pdf_pages`：读取 PDF 文本页；
- `read_docx_blocks`：读取 DOCX 正文和表格块；
- `read_xlsx_rows`：读取 XLSX 行列数据；
- `read_attachment_text`：读取普通文本和代码文件；
- PDF、论文、科学证据、Word、Excel、图片、代码解读、小白讲解和问题框定等 Skill；
- 模型能力门禁、Tool 真实活动展示和 Skill 渐进式披露的运行基础。

### 2.2 主要缺口

| 文件类型 | 当前主要缺口 |
| --- | --- |
| PDF | 主要依赖文本层；扫描件、公式、多栏版式、复杂表格和图文关系不足 |
| DOCX | 页眉页脚、脚注尾注、批注、修订、公式、图片、样式和复杂表格结构不足 |
| XLSX | 公式原文、图表、批注、透视表、命名区域、隐藏行列和样式语义不足 |
| PPTX | 缺少完整的幻灯片、备注、图表、图片和版面解析能力 |
| 图片 | 主要依赖模型视觉；缺少独立 OCR、旋转、裁切、区域放大和坐标定位 Tool |
| 代码文件 | 主要按文本和行读取；缺少 AST、符号、导入、引用关系和多文件项目地图 |

另外还需要统一附件入口与 Tool 限制。例如上传上限与实际解析 Tool 上限不应长期不一致，否则会出现“界面允许上传，但运行层无法读取”的割裂体验。

---

## 三、建议的总体分层架构

```text
L0：原生轻量层，默认安装和优先执行
├─ 现有 PDF 文本提取
├─ Calamine 工作簿读取
├─ Open XML 文档结构解析
├─ Tree-sitter 代码语法与符号解析
└─ 普通文本、元数据和文件类型识别

L1：当前模型的视觉能力，按需使用
├─ PDF 页面渲染后识别
├─ Office 内嵌图片和图表识别
├─ 截图、公式和复杂版面理解
└─ 用户当前选择的视觉模型

L2：本地高级扩展，可选安装、任务时启动
├─ Xberg 文档解析扩展
├─ Tesseract 轻量离线 OCR
└─ PaddleOCR 高质量中文 OCR Worker

L3：专业重型引擎，可选安装或独立服务
├─ Docling
├─ Marker
└─ MinerU
```

升级策略应是确定性的：

```text
识别文件类型
    ↓
先走轻量解析
    ↓
检查文本层、结构完整性和解析质量
    ↓
质量足够 ──→ 返回结构化结果
    ↓ 不足
根据当前模型能力和已安装扩展选择视觉/OCR/高级引擎
    ↓
保留页码、区域、单元格、幻灯片或代码符号来源
```

普通文档始终优先走 L0。只有文本层为空、版面复杂、公式或表格无法可靠取得，或者用户明确选择“高精度解析”时，才升级到更重的处理路径。

---

## 四、值得重点评估的高质量开源实现

### 4.1 Xberg：最值得优先制作 PoC 的统一解析候选

仓库：[xberg-io/xberg](https://github.com/xberg-io/xberg)

初步价值：

- Rust 核心，与 Mnemora 的 Tauri/Rust 后端较为契合；
- MIT 许可证；
- 覆盖 PDF、Office、图片、音频、网页、压缩包等多种格式；
- 提供 OCR、版面、表格和代码符号相关能力；
- 提供 Rust crate、CLI、REST 和 MCP 等多种使用形式；
- Cargo Feature 有机会只启用必要模块，避免一次性引入全部能力。

推荐定位：

- 统一文档解析层的优先候选；
- PPTX 和更多格式的能力补充；
- 文档结构、元数据和代码符号提取的 PoC；
- 高级 OCR 等能力的可选后端，而不是默认全部开启。

进入正式方案前必须验证：

- Windows Release 构建是否稳定；
- 最小 Feature 组合及其依赖树；
- 对安装包体积的实际影响；
- 空闲内存、冷启动和单次解析峰值；
- 中文 PDF、Office、表格、公式和代码的真实质量；
- 是否支持按页、按区块和按区域读取，避免完整文档常驻内存；
- OCR 模型下载、缓存和卸载策略；
- API 稳定性、异常文件安全性和全部传递依赖的许可证。

在完成上述基准前，不应直接用 Xberg 替换现有解析层。

### 4.2 Microsoft MarkItDown：统一接口与插件架构参考

仓库：[microsoft/markitdown](https://github.com/microsoft/markitdown)

值得借鉴：

- 将 PDF、DOCX、PPTX、XLSX、图片、HTML、CSV、JSON、XML、ZIP、EPUB、音频等内容统一转换为适合 LLM 使用的表示；
- 尽量保留标题、列表、表格和链接；
- 按格式安装可选依赖；
- 使用插件扩展新格式。

建议定位：

- 借鉴统一的 `Document → Structured Elements / Markdown` 入口；
- 借鉴格式插件、能力检测和按需依赖设计；
- 可以研究为可销毁的 Python Sidecar，但不建议默认打包进轻量核心。

主要风险是 Python 运行时、依赖体积、打包复杂度和额外进程内存。

### 4.3 Unstructured：统一 DocumentElement 数据模型参考

仓库：[Unstructured-IO/unstructured](https://github.com/Unstructured-IO/unstructured)

最值得借鉴的是统一元素模型，而不是将它直接作为核心常驻依赖：

```text
heading
paragraph
list_item
table
image
formula
code
header
footer
metadata
source_locator
```

未来 Mnemora 的不同解析 Tool 应输出统一的结构化元素，而不是所有格式最终都只返回一大段 Markdown。Markdown 可以是展示结果，但不应成为运行层唯一的数据合同。

### 4.4 Docling、Marker 与 MinerU：复杂 PDF 专业扩展

候选仓库：

- [docling-project/docling](https://github.com/docling-project/docling)
- [datalab-to/marker](https://github.com/datalab-to/marker)
- [opendatalab/MinerU](https://github.com/opendatalab/MinerU)

适合的场景：

- 扫描论文；
- 多栏和复杂版面 PDF；
- 公式、表格和图文关系；
- 高质量 PDF → Markdown / JSON；
- 批量知识库或深度笔记材料导入。

推荐定位：

- 只作为可选的高精度模式、独立 Worker 或外部服务；
- 默认不随 Mnemora 常驻启动；
- 任务结束后释放进程、模型和缓存；
- MinerU 在正式分发前尤其需要完成代码和模型权重许可证审计。

### 4.5 OCR 候选

| 项目 | 优势 | 推荐定位 |
| --- | --- | --- |
| [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) | 中文、多语言、版面和表格能力较强 | 可选的高质量 OCR Worker |
| [Tesseract](https://github.com/tesseract-ocr/tesseract) | 成熟、本地、CPU 可运行、许可证友好 | 轻量离线 OCR 选项 |
| [Umi-OCR](https://github.com/hiroi-sora/Umi-OCR) | Windows 离线 OCR 体验值得参考 | 研究其组件能否作为后台引擎复用 |

默认策略可以优先利用用户当前已有的视觉模型，减少随安装包分发本地大模型的成本；离线 OCR 则作为用户可选组件。无论选择哪一种，OCR 都必须通过真实 Tool 执行，并返回页码、区域坐标、置信度和来源。

### 4.6 Tree-sitter：代码理解的第一优先级候选

仓库：[tree-sitter/tree-sitter](https://github.com/tree-sitter/tree-sitter)

适合原因：

- Rust 生态成熟；
- 可以提取函数、类、变量、导入和代码范围；
- 支持增量解析；
- 无需执行用户代码；
- 相比完整 LSP/SCIP 更轻量；
- 可以只安装 Mnemora 首期需要的语言 Grammar。

第一阶段可优先支持 Python、JavaScript/TypeScript、Rust、C/C++、Java 和 Go。每个语言 Grammar 仍需分别审计许可证、版本和安装体积。

还可以借鉴：

- [oraios/serena](https://github.com/oraios/serena)：符号级工具、语义导航与 MCP/LSP 架构；
- [Aider-AI/aider](https://github.com/Aider-AI/aider)：Repo Map、符号排序和有限上下文选择；
- [scip-code/scip](https://github.com/scip-code/scip)：后续需要编译器级跨文件语义时再评估。

第一阶段不建议直接引入完整语言服务器体系。Mnemora 的核心场景是阅读、解读和面向初学者讲解，Tree-sitter 加轻量项目地图已经可以覆盖大量需求。

---

## 五、未来建议补充的真实 Tool

### 5.1 通用文档

```text
inspect_document
list_document_sections
read_document_elements
search_document_text
get_document_metadata
```

### 5.2 PDF、图片与 OCR

```text
render_pdf_pages
crop_document_region
rotate_document_page
ocr_document_pages
inspect_document_layout
extract_document_tables
describe_image_regions
```

### 5.3 Word 与 PowerPoint

```text
list_docx_sections
read_docx_headers_footers
read_docx_comments_revisions
list_presentation_slides
read_presentation_slide
read_presentation_notes
inspect_presentation_elements
```

### 5.4 Excel

```text
list_workbook_sheets
inspect_workbook_sheet
read_workbook_range
find_workbook_cells
list_workbook_formulas
inspect_workbook_charts
```

### 5.5 代码阅读

```text
list_project_files
search_code_text
list_code_symbols
read_code_symbol
find_symbol_references
list_code_imports
build_code_map
```

这些 Tool 不应在每一次请求中全部注入模型。运行层应根据附件类型、用户请求、当前 Skill 和模型能力，采用 Tool Search、`defer_loading` 或 Mnemora 自己的渐进式目录进行披露。

---

## 六、统一输出合同建议

未来所有文档解析器都应尽量映射到统一的 `DocumentElement`，示意字段如下：

```text
elementId
documentId
elementType
content
pageNumber
sectionPath
boundingBox
sheetName
cellRange
slideNumber
codeSymbol
confidence
metadata
sourceHash
```

不同格式只填写适用字段。例如：

- PDF 段落保留页码和坐标；
- Excel 表格保留工作表和单元格范围；
- PPTX 元素保留幻灯片编号和版面位置；
- 代码符号保留文件、语言、符号名和行范围；
- OCR 结果保留置信度和图像区域。

模型获得的是经过范围控制的元素集合；前端可以根据 `source_locator` 回到原始页面、单元格、幻灯片或代码行。这样才能支持可验证引用、深度笔记证据和局部重读。

---

## 七、附件能力门禁需要随之统一

“是否允许上传某种附件”不应只判断模型名称，而应判断从附件到模型的完整执行链是否成立。

示例：

```text
图片可用 =
模型原生支持视觉
或
模型支持 Tool 且已安装可用的图片/OCR Tool
```

```text
PDF 可用 =
Provider 明确支持原生 PDF
或
模型支持 Tool 且已安装 PDF 解析 Tool
```

```text
XLSX 可用 =
模型支持 Tool 且已安装工作簿 Tool
```

完整链路不成立时，输入框应禁用对应附件或在上传前明确阻止，并提示用户切换支持相应能力的模型、安装扩展或移除附件。不能允许上传以后再让模型依据文件名猜测内容。

---

## 八、Skill 的未来增强原则

未来可以继续引入高质量的文档和代码阅读 Skill，但必须遵循以下边界：

1. Skill 负责方法论，不负责伪造文件读取能力。
2. 只有真实加载 Skill 并产生 `skillActivated` 事件，前端才显示技能活动。
3. 只有运行层真实执行 Tool 并产生 Tool Trace，前端才显示工具调用和结果。
4. 从 GitHub 引入 Skill 时，必须记录作者、仓库、路径、Commit、许可证和本地修改。
5. Skill 中引用的外部工具名称，必须映射到 Mnemora 已经实现的真实 Tool。
6. 默认安装不等于默认加载；只先披露名称、简介、适用条件和能力依赖，选中后再加载正文。
7. 文档 Skill 应要求模型保留来源定位、识别证据不足和 OCR 低置信度，不能把不确定内容写成事实。
8. 代码 Skill 的产品重心应是架构理解、执行流程解读、概念讲解和面向初学者说明，而不是默认进入 Code Review。

---

## 九、内存、流畅性和安全边界

该候选方向必须继续服从 Plan07 的资源治理目标。高质量解析能力不能以持续增加 WebView2 或主进程常驻内存为代价。

基本约束：

- 普通解析器默认延迟加载；
- 大文件按页、按区块、按区域和按符号读取；
- 不把完整 PDF、工作簿或代码仓库一次性复制进 React 状态；
- 解析结果采用容量、数量和生命周期受控的缓存；
- OCR、Python、ONNX 和模型类引擎放入独立 Worker/Sidecar；
- 重型 Worker 只在任务执行时启动，任务结束或超时后销毁；
- Worker 崩溃不得拖垮主应用，未完成任务需要返回可解释错误；
- 所有解析器必须设置文件大小、页数、解压规模、执行时间和输出长度上限；
- 压缩包、XML、Office 和异常 PDF 需要防止解压炸弹、实体扩展和恶意构造文件；
- 未经验证的文档内容属于不可信输入，不能通过内容诱导运行层绕过权限或自动执行代码。

是否把某个能力迁入独立 Worker，应由 Release 数据决定：如果其依赖明显增加常驻内存，或者任务结束后仍残留大量不可回收资源，就不能放在主进程长期存活。

---

## 十、候选采用矩阵

| 候选 | 初步定位 | 当前建议 |
| --- | --- | --- |
| Xberg | Rust 统一解析和多格式扩展 | 第一优先级 PoC，不直接全面替换 |
| Tree-sitter | 代码 AST、符号和项目地图 | 第一阶段核心候选 |
| MarkItDown | 统一转换入口和插件架构 | 重点借鉴；可评估 Sidecar |
| Unstructured | 统一元素数据合同 | 借鉴数据模型，不作为默认常驻核心 |
| Tesseract | 轻量离线 OCR | 可选组件候选 |
| PaddleOCR | 中文和复杂版面 OCR | 可选高质量 Worker 候选 |
| Docling | 复杂文档和结构化管线 | 专业扩展候选 |
| Marker | 高质量 PDF 转 Markdown/JSON | 专业 PDF 扩展候选 |
| MinerU | 中文论文、公式和表格 | 许可证审计后再决定 |
| Serena | 语义代码工具架构 | 借鉴 Tool 设计，不默认常驻 |
| Aider | Repo Map 和上下文选择 | 借鉴项目地图算法 |
| SCIP/LSP | 编译器级跨文件语义 | 后期有真实需求时再评估 |

---

## 十一、未来转为正式 Plan 前必须完成的工作

### 11.1 重新调研与许可证审计

- 固定每个候选的版本或 Commit；
- 审计项目本身、可选模块、语言 Grammar、模型权重和传递依赖；
- 确认是否允许修改、内置和随安装包再分发；
- 建立第三方来源、许可证和版本更新记录。

### 11.2 Xberg / Tree-sitter 最小 PoC

- 构建最小 Rust Feature 组合；
- 支持少量代表性格式和语言；
- 验证能否按页、按区块和按符号读取；
- 测量 Release 安装体积、冷启动、空闲内存和峰值内存；
- 对比现有 Tool 的质量、速度和错误率。

### 11.3 重型 Sidecar 对照实验

对 MarkItDown、Docling、Marker、PaddleOCR 等建立同一套样本：

- 中文文本 PDF；
- 扫描 PDF；
- 双栏论文；
- 复杂表格和公式；
- 含图片、批注和修订的 DOCX；
- 含公式、合并单元格、图表和多 Sheet 的 XLSX；
- 含备注、图表和复杂版式的 PPTX。

统一测量：

- 安装体积；
- 冷启动时间；
- 单任务延迟；
- idle / peak memory；
- 解析正确率和来源定位能力；
- 中文质量；
- 离线可用性；
- Worker 销毁后的资源回收情况。

### 11.4 正式 Plan 的建议阶段

如果未来决定实施，可将正式计划拆为：

```text
P0：统一能力合同、DocumentElement 和附件门禁
P1：Tree-sitter 代码符号与轻量项目地图
P2：Xberg 最小解析 PoC 和现有 Tool 对照
P3：PDF 页面渲染、区域裁切与视觉/OCR 回退
P4：PPTX、DOCX、XLSX 高级结构增强
P5：可选重型 Sidecar、扩展安装和生命周期治理
P6：真实语料评测、Release 内存基线和 CI 门禁
```

正式 Plan 必须明确版本范围、数据合同、迁移方式、工具权限、错误状态、UI 入口、测试样本、性能预算和逐阶段验收条件。

---

## 十二、当前不做的决定

在该候选文档阶段，不作出以下承诺：

- 不确定最终一定采用 Xberg；
- 不确定默认内置哪一种 OCR；
- 不把 Python 或机器学习环境直接加入默认安装包；
- 不一次性支持所有文件格式和编程语言；
- 不让 Skill 代替尚未实现的 Tool；
- 不为了功能数量牺牲 Mnemora 的低内存、流畅性和稳定性；
- 不在缺少真实 Release 基准时决定主进程、Worker 或 Sidecar 的最终归属。

该文档的作用，是保存已完成的调研方向和架构判断。未来真正启动优化时，再围绕实际需求、当时的项目状态和真实数据进行逐项讨论，并将确认后的方案升级为带正式序号的 Plan。
