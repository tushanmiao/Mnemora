//! Mnemora 通用基础设置。
//!
//! 这里保存外观、行为、个人资料和 Chat 默认行为，不保存 Provider API Key。
//! `ModelSettings` 负责模型服务结构；`AppSettings` 负责应用级体验，两者版本独立。

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::memory::MemorySettings;

pub const CURRENT_APP_SETTINGS_VERSION: u32 = 18;
pub const DEFAULT_GLOBAL_SYSTEM_PROMPT: &str = concat!(
    "你是 Mnemora 的学习与研究助手。\n",
    "优先直接回答问题，并根据复杂度使用清晰的标题、列表、表格或代码块。\n",
    "严格区分已知事实、用户材料中的证据、合理推断和仍需确认的内容；没有依据时明确说明。\n",
    "处理 PDF、图片或附件时，只根据实际收到的内容回答，不编造来源、页码、工具结果或已执行操作。\n",
    "技能只提供工作方法，不扩大应用权限；遵守用户的权限设置和工具结果。"
);
const MAX_AVATAR_DATA_URL_BYTES: usize = 3 * 1024 * 1024;
const MAX_THEME_BACKGROUND_CSS_BYTES: usize = 1_500_000;
const MIN_SURFACE_OPACITY: u8 = 72;
const MAX_SURFACE_OPACITY: u8 = 100;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterfaceLanguage {
    #[default]
    Zh,
    En,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteEditorSettings {
    pub default_mode: String,
    pub autosave_enabled: bool,
    pub autosave_delay_ms: u32,
    pub line_numbers: bool,
    pub word_wrap: bool,
    pub tab_size: u8,
    pub focus_mode: bool,
    pub typewriter_mode: bool,
    pub spellcheck: bool,
    pub render_policy: String,
}

impl Default for NoteEditorSettings {
    fn default() -> Self {
        Self {
            default_mode: "live".into(),
            autosave_enabled: true,
            autosave_delay_ms: 700,
            line_numbers: true,
            word_wrap: true,
            tab_size: 2,
            focus_mode: false,
            typewriter_mode: false,
            spellcheck: false,
            render_policy: "auto".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemePreset {
    #[default]
    Mnemora,
    Forest,
    Ocean,
    Rose,
    Paper,
    Graphite,
    HighContrast,
    /// 光家族：冷白天光 / 钨丝暖光。
    Dawn,
    Lamp,
    /// 纸家族：宣纸（墨与朱砂）/ 蓝晒（普鲁士蓝）。
    Xuan,
    Cyanotype,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeColor {
    #[default]
    Neutral,
    Warm,
    Cool,
    Rose,
    Amber,
    Violet,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FontPreset {
    #[default]
    System,
    Academic,
    Custom,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChineseFontFamily {
    #[default]
    System,
    MicrosoftYaHei,
    Simsun,
    NotoSansCjk,
    NotoSerifCjk,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LatinFontFamily {
    #[default]
    System,
    SegoeUi,
    Inter,
    TimesNewRoman,
    Georgia,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeBackgroundSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub css: String,
    #[serde(default = "default_surface_opacity")]
    pub surface_opacity: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub show_on_startup: bool,
    #[serde(default = "default_true")]
    pub always_on_top: bool,
    #[serde(default)]
    pub click_through: bool,
    #[serde(default = "default_true")]
    pub locked: bool,
    #[serde(default = "default_pet_size")]
    pub size: u16,
    #[serde(default = "default_pet_opacity")]
    pub opacity: u8,
    #[serde(default = "default_true")]
    pub speech_bubbles: bool,
    #[serde(default)]
    pub reduced_motion: bool,
    #[serde(default = "default_true")]
    pub task_events: bool,
    #[serde(default = "default_pet_id")]
    pub selected_pet_id: String,
    #[serde(default)]
    pub position_x: Option<f64>,
    #[serde(default)]
    pub position_y: Option<f64>,
}

impl Default for PetSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            show_on_startup: false,
            always_on_top: true,
            click_through: false,
            locked: true,
            size: default_pet_size(),
            opacity: default_pet_opacity(),
            speech_bubbles: true,
            reduced_motion: false,
            task_events: true,
            selected_pet_id: default_pet_id(),
            position_x: None,
            position_y: None,
        }
    }
}

impl Default for ThemeBackgroundSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            css: String::new(),
            surface_opacity: default_surface_opacity(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseLanguage {
    #[default]
    FollowInput,
    Zh,
    ZhHant,
    En,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateProxyMode {
    #[default]
    System,
    Direct,
    Manual,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProxySettings {
    #[serde(default)]
    pub mode: UpdateProxyMode,
    #[serde(default)]
    pub url: String,
}

impl UpdateProxySettings {
    pub fn manual_url(&self) -> Result<Url, String> {
        let value = self.url.trim();
        if value.is_empty() {
            return Err("Manual network proxy URL is required".to_string());
        }
        if value.len() > 2_048 {
            return Err("Network proxy URL is too long".to_string());
        }
        let normalized = if value.contains("://") {
            value.to_string()
        } else {
            format!("http://{value}")
        };
        let url = Url::parse(&normalized)
            .map_err(|error| format!("Invalid network proxy URL: {error}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("Network proxy must use HTTP or HTTPS".to_string());
        }
        if url.host_str().is_none() {
            return Err("Network proxy URL must include a host".to_string());
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(
                "Network proxy credentials cannot be stored in application settings".to_string(),
            );
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err("Network proxy URL cannot include a query or fragment".to_string());
        }
        Ok(url)
    }
}

/// 默认知识范围。Chat、Memory 和临时附件不属于任何知识源范围。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeScope {
    #[default]
    Library,
    CurrentLiterature,
    CurrentNote,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeRetrievalMode {
    #[default]
    Lexical,
    Vector,
    Hybrid,
}

impl KnowledgeRetrievalMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Vector => "vector",
            Self::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeCloudConsentMode {
    /// 每次需要把本地文献发送到云端时都询问。
    #[default]
    Ask,
    /// 用户对单篇文献作出的持续同意；具体记录由知识库域保存。
    Document,
    /// 用户对当前本地知识库作出的全局同意；具体记录由知识库域保存。
    Global,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeBatchStrategy {
    #[default]
    PageBatches,
    ManualSplit,
    Reject,
}

/// PDF/Markdown 知识库的非敏感策略。
///
/// MinerU Token 不得写入 AppSettings；桌面端后续通过 SecretStore 管理。索引
/// 数据是可重建的派生数据，不能取代现有 library 业务表。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub auto_retrieve: bool,
    #[serde(default)]
    pub default_scope: KnowledgeScope,
    #[serde(default)]
    pub retrieval_mode: KnowledgeRetrievalMode,
    #[serde(default)]
    pub embedding_provider: String,
    #[serde(default)]
    pub embedding_model: String,
    #[serde(default = "default_knowledge_chunk_target")]
    pub chunk_target_chars: u32,
    #[serde(default = "default_knowledge_chunk_max")]
    pub chunk_max_chars: u32,
    #[serde(default = "default_knowledge_chunk_overlap")]
    pub chunk_overlap_chars: u32,
    #[serde(default = "default_knowledge_top_k")]
    pub top_k: u8,
    #[serde(default = "default_knowledge_context_bytes")]
    pub context_max_bytes: u32,
    #[serde(default)]
    pub include_annotations: bool,
    #[serde(default = "default_true")]
    pub grounded_work: bool,
    #[serde(default = "default_true")]
    pub mineru_cloud_enabled: bool,
    #[serde(default = "default_mineru_endpoint")]
    pub mineru_endpoint: String,
    #[serde(default = "default_mineru_model")]
    pub mineru_model: String,
    #[serde(default = "default_true")]
    pub mineru_ocr_enabled: bool,
    #[serde(default = "default_true")]
    pub mineru_formula_enabled: bool,
    #[serde(default = "default_true")]
    pub mineru_table_enabled: bool,
    #[serde(default = "default_true")]
    pub mineru_figure_enabled: bool,
    #[serde(default = "default_mineru_language")]
    pub mineru_language: String,
    #[serde(default)]
    pub mineru_consent_mode: KnowledgeCloudConsentMode,
    #[serde(default)]
    pub auto_parse_imported_pdf: bool,
    #[serde(default = "default_true")]
    pub allow_local_text_fallback: bool,
    #[serde(default = "default_remote_page_budget")]
    pub remote_page_budget_per_day: u32,
    #[serde(default = "default_remote_task_budget")]
    pub remote_task_budget_per_day: u32,
    #[serde(default)]
    pub batch_strategy: KnowledgeBatchStrategy,
    #[serde(default = "default_network_timeout_seconds")]
    pub network_timeout_seconds: u16,
    #[serde(default = "default_knowledge_concurrency")]
    pub index_concurrency: u8,
    #[serde(default = "default_true")]
    pub markdown_assets_enabled: bool,
    #[serde(default)]
    pub embedding_enabled: bool,
    #[serde(default)]
    pub hybrid_enabled: bool,
    #[serde(default)]
    pub allow_remote_embedding: bool,
    #[serde(default)]
    pub external_mcp_enabled: bool,
    #[serde(default)]
    pub debug_retrieval: bool,
}

impl Default for KnowledgeSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_retrieve: false,
            default_scope: KnowledgeScope::Library,
            retrieval_mode: KnowledgeRetrievalMode::Lexical,
            embedding_provider: String::new(),
            embedding_model: String::new(),
            chunk_target_chars: default_knowledge_chunk_target(),
            chunk_max_chars: default_knowledge_chunk_max(),
            chunk_overlap_chars: default_knowledge_chunk_overlap(),
            top_k: default_knowledge_top_k(),
            context_max_bytes: default_knowledge_context_bytes(),
            include_annotations: false,
            grounded_work: true,
            mineru_cloud_enabled: true,
            mineru_endpoint: default_mineru_endpoint(),
            mineru_model: default_mineru_model(),
            mineru_ocr_enabled: true,
            mineru_formula_enabled: true,
            mineru_table_enabled: true,
            mineru_figure_enabled: true,
            mineru_language: default_mineru_language(),
            mineru_consent_mode: KnowledgeCloudConsentMode::Ask,
            auto_parse_imported_pdf: false,
            allow_local_text_fallback: true,
            remote_page_budget_per_day: default_remote_page_budget(),
            remote_task_budget_per_day: default_remote_task_budget(),
            batch_strategy: KnowledgeBatchStrategy::PageBatches,
            network_timeout_seconds: default_network_timeout_seconds(),
            index_concurrency: default_knowledge_concurrency(),
            markdown_assets_enabled: true,
            embedding_enabled: false,
            hybrid_enabled: false,
            allow_remote_embedding: false,
            external_mcp_enabled: false,
            debug_retrieval: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub interface_language: InterfaceLanguage,
    #[serde(default)]
    pub theme: ThemeMode,
    #[serde(default)]
    pub theme_preset: ThemePreset,
    #[serde(default)]
    pub theme_color: ThemeColor,
    #[serde(default)]
    pub theme_background: ThemeBackgroundSettings,
    #[serde(default = "default_font_size")]
    pub font_size: u8,
    #[serde(default)]
    pub letter_spacing: f32,
    #[serde(default)]
    pub font_preset: FontPreset,
    #[serde(default)]
    pub chinese_font_family: ChineseFontFamily,
    #[serde(default)]
    pub latin_font_family: LatinFontFamily,
    #[serde(default = "default_note_font_size")]
    pub note_font_size: u8,
    #[serde(default = "default_note_line_height")]
    pub note_line_height: f32,
    #[serde(default)]
    pub note_font_preset: FontPreset,
    #[serde(default)]
    pub note_chinese_font_family: ChineseFontFamily,
    #[serde(default)]
    pub note_latin_font_family: LatinFontFamily,
    #[serde(default)]
    pub note_editor: NoteEditorSettings,
    #[serde(default)]
    pub launch_at_startup: bool,
    #[serde(default = "default_true")]
    pub retry_enabled: bool,
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u8,
    /// Agent 业务轮数；达到上限后运行层仍保留一次无工具最终汇总调用。
    #[serde(default = "default_agent_max_rounds")]
    pub agent_max_rounds: u16,
    #[serde(default)]
    pub user_display_name: String,
    #[serde(default)]
    pub user_avatar: String,
    #[serde(default)]
    pub working_directory: String,
    #[serde(default = "default_true")]
    pub stream_enabled: bool,
    /// 深度笔记的模型调用是否走流式。
    ///
    /// 与 `stream_enabled` 无关：后者只被前端用来决定调哪个聊天命令（为了 UI 逐字），
    /// 后端从不读它。这个开关的目的是**保活** —— 非流式请求在生成期间连接静默，
    /// 中转站的 idle 超时通常远短于长文生成，会导致主动撞上 504。
    ///
    /// 默认开启。个别中转站对流式与非流式的路由策略不同、少数上游不支持流式，
    /// 因此保留这个关闭入口；运行期若流式失败也会自动回落非流式并记一次告警。
    #[serde(default = "default_true")]
    pub deep_note_stream_keepalive: bool,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default)]
    pub response_language: ResponseLanguage,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub request_debug_enabled: bool,
    #[serde(default = "default_true")]
    pub show_chat_task_progress: bool,
    #[serde(default)]
    pub pet: PetSettings,
    #[serde(default)]
    /// 历史 JSON 键仍为 updateProxy；现在作为网页工具与更新器共用的出站代理策略。
    pub update_proxy: UpdateProxySettings,
    #[serde(default)]
    pub memory: MemorySettings,
    #[serde(default)]
    pub knowledge: KnowledgeSettings,
}

fn current_version() -> u32 {
    CURRENT_APP_SETTINGS_VERSION
}

fn default_true() -> bool {
    true
}

fn default_retry_attempts() -> u8 {
    5
}

fn default_agent_max_rounds() -> u16 {
    20
}

fn default_font_size() -> u8 {
    14
}

fn default_note_font_size() -> u8 {
    16
}

fn default_note_line_height() -> f32 {
    1.85
}

fn default_surface_opacity() -> u8 {
    92
}

fn default_pet_size() -> u16 {
    176
}

fn default_pet_opacity() -> u8 {
    96
}

fn default_pet_id() -> String {
    "mimo".to_string()
}

fn default_max_output_tokens() -> u32 {
    32_768
}

fn default_knowledge_chunk_target() -> u32 {
    1_600
}

fn default_knowledge_chunk_max() -> u32 {
    2_400
}

fn default_knowledge_chunk_overlap() -> u32 {
    200
}

fn default_knowledge_top_k() -> u8 {
    8
}

fn default_knowledge_context_bytes() -> u32 {
    64 * 1024
}

fn default_mineru_endpoint() -> String {
    "https://mineru.net/api/v4".to_string()
}

fn default_mineru_model() -> String {
    "vlm".to_string()
}

fn default_mineru_language() -> String {
    "ch".to_string()
}

fn default_remote_page_budget() -> u32 {
    1_000
}

fn default_remote_task_budget() -> u32 {
    20
}

fn default_network_timeout_seconds() -> u16 {
    120
}

fn default_knowledge_concurrency() -> u8 {
    2
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: CURRENT_APP_SETTINGS_VERSION,
            interface_language: InterfaceLanguage::Zh,
            theme: ThemeMode::System,
            theme_preset: ThemePreset::Mnemora,
            theme_color: ThemeColor::Neutral,
            theme_background: ThemeBackgroundSettings::default(),
            font_size: 14,
            letter_spacing: 0.0,
            font_preset: FontPreset::System,
            chinese_font_family: ChineseFontFamily::System,
            latin_font_family: LatinFontFamily::System,
            note_font_size: 16,
            note_line_height: 1.85,
            note_font_preset: FontPreset::System,
            note_chinese_font_family: ChineseFontFamily::System,
            note_latin_font_family: LatinFontFamily::System,
            note_editor: NoteEditorSettings::default(),
            launch_at_startup: false,
            retry_enabled: true,
            retry_attempts: 5,
            agent_max_rounds: 20,
            user_display_name: String::new(),
            user_avatar: String::new(),
            working_directory: String::new(),
            stream_enabled: true,
            deep_note_stream_keepalive: true,
            thinking_enabled: false,
            max_output_tokens: 32_768,
            response_language: ResponseLanguage::FollowInput,
            system_prompt: DEFAULT_GLOBAL_SYSTEM_PROMPT.to_string(),
            request_debug_enabled: false,
            show_chat_task_progress: true,
            pet: PetSettings::default(),
            update_proxy: UpdateProxySettings::default(),
            memory: MemorySettings::default(),
            knowledge: KnowledgeSettings::default(),
        }
    }
}

impl AppSettings {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        let source_version = self.version;
        if !["live", "source", "read"].contains(&self.note_editor.default_mode.as_str())
            || !["auto", "sourceOnly"].contains(&self.note_editor.render_policy.as_str())
            || !(300..=5000).contains(&self.note_editor.autosave_delay_ms)
            || ![2, 4, 8].contains(&self.note_editor.tab_size)
        {
            return Err("Invalid note editor settings".into());
        }
        if source_version < 18 {
            self.note_editor.default_mode = "source".into();
        }
        if self.version > CURRENT_APP_SETTINGS_VERSION {
            return Err(format!(
                "App settings version {} is newer than supported version {}",
                self.version, CURRENT_APP_SETTINGS_VERSION
            ));
        }
        if source_version < 3 && self.retry_attempts == 1 {
            self.retry_attempts = 5;
        }
        if source_version < 8 && self.system_prompt.trim().is_empty() {
            self.system_prompt = DEFAULT_GLOBAL_SYSTEM_PROMPT.to_string();
        }
        self.theme_color = ThemeColor::Neutral;
        self.version = CURRENT_APP_SETTINGS_VERSION;
        self.user_display_name = self.user_display_name.trim().to_string();
        self.user_avatar = self.user_avatar.trim().to_string();
        self.working_directory = self.working_directory.trim().to_string();
        self.system_prompt = self.system_prompt.trim().to_string();
        self.theme_background.css = self.theme_background.css.trim().to_string();
        self.update_proxy.url = self.update_proxy.url.trim().to_string();
        if source_version < 14 {
            self.pet = PetSettings::default();
        } else if source_version < 15 {
            self.pet.selected_pet_id = default_pet_id();
        }
        if source_version < 16 {
            self.pet.locked = true;
        }
        if source_version < 17 {
            // 知识库配置在 v17 首次加入；旧配置不能因为反序列化缺字段而
            // 获得隐式的云端上传或远程 embedding 权限。
            self.knowledge = KnowledgeSettings::default();
        }
        self.pet.selected_pet_id = self.pet.selected_pet_id.trim().to_string();
        self.knowledge.embedding_provider = self.knowledge.embedding_provider.trim().to_string();
        self.knowledge.embedding_model = self.knowledge.embedding_model.trim().to_string();
        self.knowledge.mineru_endpoint = self.knowledge.mineru_endpoint.trim().to_string();
        self.knowledge.mineru_model = self.knowledge.mineru_model.trim().to_ascii_lowercase();
        self.knowledge.mineru_language = self.knowledge.mineru_language.trim().to_string();

        if self.user_display_name.chars().count() > 100 {
            return Err("User display name is too long".to_string());
        }
        if self.user_avatar.len() > MAX_AVATAR_DATA_URL_BYTES {
            return Err("User avatar image is too large".to_string());
        }
        if !self.user_avatar.is_empty() {
            let safe_data_image = [
                "data:image/png;base64,",
                "data:image/jpeg;base64,",
                "data:image/webp;base64,",
                "data:image/gif;base64,",
            ]
            .iter()
            .any(|prefix| self.user_avatar.starts_with(prefix));
            let legacy_http_image = Url::parse(&self.user_avatar)
                .ok()
                .is_some_and(|url| matches!(url.scheme(), "http" | "https"));
            if !safe_data_image && !legacy_http_image {
                return Err(
                    "User avatar must be an uploaded PNG, JPEG, WebP or GIF image".to_string(),
                );
            }
        }
        if self.working_directory.len() > 32_768 {
            return Err("Working directory is too long".to_string());
        }
        if !(12..=28).contains(&self.font_size) {
            return Err("Font size must be between 12 and 28".to_string());
        }
        if !(12..=32).contains(&self.note_font_size) {
            return Err("Note font size must be between 12 and 32".to_string());
        }
        if !self.note_line_height.is_finite() || !(1.3..=2.4).contains(&self.note_line_height) {
            return Err("Note line height must be between 1.3 and 2.4".to_string());
        }
        if !self.letter_spacing.is_finite() || !(0.0..=1.5).contains(&self.letter_spacing) {
            return Err("Text letter spacing must be between 0 and 1.5 px".to_string());
        }
        if !(MIN_SURFACE_OPACITY..=MAX_SURFACE_OPACITY)
            .contains(&self.theme_background.surface_opacity)
        {
            return Err(format!(
                "Theme surface opacity must be between {MIN_SURFACE_OPACITY} and {MAX_SURFACE_OPACITY}"
            ));
        }
        if self.theme_background.enabled && self.theme_background.css.is_empty() {
            return Err("Enabled theme background requires a CSS value".to_string());
        }
        if !self.theme_background.css.is_empty() {
            validate_theme_background_css(&self.theme_background.css)?;
        }
        if !(1..=5).contains(&self.retry_attempts) {
            return Err("Retry attempts must be between 1 and 5".to_string());
        }
        if !matches!(self.agent_max_rounds, 5 | 10 | 20 | 50 | 100) {
            return Err("Agent maximum rounds must be one of 5, 10, 20, 50, or 100".to_string());
        }
        if !(256..=131_072).contains(&self.max_output_tokens) {
            return Err("Maximum output tokens must be between 256 and 131072".to_string());
        }
        if self.system_prompt.len() > 256 * 1024 {
            return Err("System Prompt is too long".to_string());
        }
        if !(120..=280).contains(&self.pet.size) {
            return Err("Pet size must be between 120 and 280".to_string());
        }
        if !(40..=100).contains(&self.pet.opacity) {
            return Err("Pet opacity must be between 40 and 100".to_string());
        }
        if self.pet.selected_pet_id.len() > 96
            || (!self.pet.selected_pet_id.is_empty()
                && (!self.pet.selected_pet_id.chars().all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
                }) || self.pet.selected_pet_id.starts_with('-')
                    || self.pet.selected_pet_id.ends_with('-')))
        {
            return Err("Pet id must be a lowercase slug".to_string());
        }
        if self.pet.selected_pet_id.is_empty() {
            self.pet.selected_pet_id = default_pet_id();
        }
        if self
            .pet
            .position_x
            .is_some_and(|value| !value.is_finite() || value.abs() > 100_000.0)
            || self
                .pet
                .position_y
                .is_some_and(|value| !value.is_finite() || value.abs() > 100_000.0)
        {
            return Err("Pet window position is invalid".to_string());
        }
        if !(256..=8_192).contains(&self.knowledge.chunk_target_chars) {
            return Err(
                "Knowledge chunk target must be between 256 and 8192 characters".to_string(),
            );
        }
        if self.knowledge.chunk_max_chars < self.knowledge.chunk_target_chars
            || self.knowledge.chunk_max_chars > 16_384
        {
            return Err(
                "Knowledge chunk maximum must be at least the target and no more than 16384 characters"
                    .to_string(),
            );
        }
        if self.knowledge.chunk_overlap_chars > self.knowledge.chunk_target_chars / 2 {
            return Err("Knowledge chunk overlap cannot exceed half of the target".to_string());
        }
        if !(1..=50).contains(&self.knowledge.top_k) {
            return Err("Knowledge top-k must be between 1 and 50".to_string());
        }
        if !(4 * 1024..=256 * 1024).contains(&self.knowledge.context_max_bytes) {
            return Err("Knowledge context bytes must be between 4096 and 262144".to_string());
        }
        if !(1..=4).contains(&self.knowledge.index_concurrency) {
            return Err("Knowledge index concurrency must be between 1 and 4".to_string());
        }
        if !(30..=600).contains(&self.knowledge.network_timeout_seconds) {
            return Err("Knowledge network timeout must be between 30 and 600 seconds".to_string());
        }
        if !(1..=100_000).contains(&self.knowledge.remote_page_budget_per_day) {
            return Err("Knowledge daily page budget must be between 1 and 100000".to_string());
        }
        if !(1..=1_000).contains(&self.knowledge.remote_task_budget_per_day) {
            return Err("Knowledge daily task budget must be between 1 and 1000".to_string());
        }
        if self.knowledge.embedding_provider.len() > 256
            || self.knowledge.embedding_model.len() > 256
        {
            return Err("Knowledge embedding provider or model is too long".to_string());
        }
        if self.knowledge.mineru_model != "vlm" && self.knowledge.mineru_model != "pipeline" {
            return Err("MinerU model must be vlm or pipeline".to_string());
        }
        if self.knowledge.mineru_language.is_empty()
            || self.knowledge.mineru_language.len() > 32
            || !self.knowledge.mineru_language.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, ',' | '+' | '-' | '_' | ' ')
            })
        {
            return Err("MinerU language list is invalid".to_string());
        }
        if self.knowledge.mineru_endpoint.len() > 2_048 {
            return Err("MinerU endpoint is too long".to_string());
        }
        let endpoint = Url::parse(&self.knowledge.mineru_endpoint)
            .map_err(|error| format!("Invalid MinerU endpoint: {error}"))?;
        if endpoint.scheme() != "https" {
            return Err("MinerU endpoint must use HTTPS".to_string());
        }
        if endpoint.host_str().is_none() {
            return Err("MinerU endpoint must include a host".to_string());
        }
        if !endpoint.username().is_empty() || endpoint.password().is_some() {
            return Err("MinerU endpoint cannot contain credentials".to_string());
        }
        if endpoint.query().is_some() || endpoint.fragment().is_some() {
            return Err("MinerU endpoint cannot include a query or fragment".to_string());
        }
        if matches!(
            self.knowledge.retrieval_mode,
            KnowledgeRetrievalMode::Vector | KnowledgeRetrievalMode::Hybrid
        ) && !self.knowledge.embedding_enabled
        {
            return Err("Vector or hybrid retrieval requires embedding to be enabled".to_string());
        }
        if self.knowledge.hybrid_enabled && !self.knowledge.embedding_enabled {
            return Err("Hybrid retrieval cannot be enabled without embedding".to_string());
        }
        if !self.update_proxy.url.is_empty() {
            self.update_proxy.url = self.update_proxy.manual_url()?.to_string();
        } else if self.update_proxy.mode == UpdateProxyMode::Manual {
            return Err("Manual network proxy URL is required".to_string());
        }
        Ok(self)
    }
}

fn validate_theme_background_css(value: &str) -> Result<(), String> {
    if value.len() > MAX_THEME_BACKGROUND_CSS_BYTES {
        return Err(format!(
            "Theme background CSS cannot exceed {MAX_THEME_BACKGROUND_CSS_BYTES} bytes"
        ));
    }
    if value.chars().any(|character| {
        !(character.is_ascii_alphanumeric()
            || character.is_ascii_whitespace()
            || matches!(
                character,
                '#' | '('
                    | ')'
                    | ','
                    | '.'
                    | '%'
                    | '+'
                    | '-'
                    | '/'
                    | ':'
                    | '_'
                    | '\''
                    | '"'
                    | '?'
                    | '&'
                    | '='
            ))
    }) {
        return Err("Theme background CSS contains unsupported characters".to_string());
    }

    let lower = value.to_ascii_lowercase();
    for blocked in [
        "paint(",
        "element(",
        "expression(",
        "@import",
        "javascript",
        "/*",
        "*/",
    ] {
        if lower.contains(blocked) {
            return Err("Theme background CSS cannot load resources or execute code".to_string());
        }
    }

    let mut depth = 0i32;
    for character in value.chars() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err("Theme background CSS has unbalanced parentheses".to_string());
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("Theme background CSS has unbalanced parentheses".to_string());
    }

    let allowed_functions = [
        "linear-gradient",
        "repeating-linear-gradient",
        "radial-gradient",
        "repeating-radial-gradient",
        "conic-gradient",
        "repeating-conic-gradient",
        "rgb",
        "rgba",
        "hsl",
        "hsla",
        "hwb",
        "lab",
        "lch",
        "oklab",
        "oklch",
        "color",
        "color-mix",
    ];
    for (index, character) in value.char_indices() {
        if character != '(' {
            continue;
        }
        let prefix = &value[..index];
        let name = prefix
            .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !allowed_functions.contains(&name.as_str())
            && !matches!(name.as_str(), "url" | "image-set" | "cross-fade")
        {
            return Err(format!("Unsupported theme background function: {name}"));
        }
    }
    validate_theme_background_urls(value)?;
    Ok(())
}

fn validate_theme_background_urls(value: &str) -> Result<(), String> {
    let lower = value.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative_start) = lower[cursor..].find("url(") {
        let start = cursor + relative_start + 4;
        let Some(relative_end) = lower[start..].find(')') else {
            return Err("Theme background URL has unbalanced parentheses".to_string());
        };
        let end = start + relative_end;
        let raw = value[start..end].trim().trim_matches(['\'', '"']);
        if raw.is_empty() {
            return Err("Theme background URL cannot be empty".to_string());
        }
        if raw.len() > 1_500_000 {
            return Err("Embedded theme background image is too large".to_string());
        }
        if raw.to_ascii_lowercase().starts_with("data:") {
            let data = raw.to_ascii_lowercase();
            if !data.starts_with("data:image/") || !data.contains(";base64,") {
                return Err("Theme background data URLs must be base64 images".to_string());
            }
        } else {
            let url = reqwest::Url::parse(raw)
                .map_err(|_| "Theme background URL must be absolute".to_string())?;
            if !matches!(url.scheme(), "http" | "https" | "asset" | "blob") {
                return Err("Theme background URL scheme is not allowed".to_string());
            }
            if !matches!(url.scheme(), "asset" | "blob") && url.host_str().is_none() {
                return Err("Theme background URL must include a host".to_string());
            }
            if !url.username().is_empty() || url.password().is_some() {
                return Err("Theme background URL cannot contain credentials".to_string());
            }
        }
        cursor = end + 1;
    }
    if lower.contains("javascript:") || lower.contains("file:") {
        return Err("Theme background cannot execute scripts or read file URLs".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AppSettings, KnowledgeRetrievalMode, KnowledgeSettings, ThemeColor, ThemeMode, ThemePreset,
        UpdateProxyMode, CURRENT_APP_SETTINGS_VERSION,
    };

    #[test]
    fn defaults_are_versioned_and_streaming_is_enabled() {
        let settings = AppSettings::default();
        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
        assert_eq!(settings.theme, ThemeMode::System);
        assert_eq!(settings.theme_preset, ThemePreset::Mnemora);
        assert!(settings.stream_enabled);
        // 流式保活默认开启：中转站的 idle 超时是深度笔记 504 的主要来源。
        assert!(settings.deep_note_stream_keepalive);
        assert!(!settings.request_debug_enabled);
        assert!(settings.show_chat_task_progress);
        assert!(!settings.pet.enabled);
        assert!(settings.pet.always_on_top);
        assert!(settings.pet.locked);
        assert_eq!(settings.pet.size, 176);
        assert_eq!(settings.retry_attempts, 5);
        assert_eq!(settings.agent_max_rounds, 20);
        assert_eq!(settings.font_size, 14);
        assert_eq!(settings.note_font_size, 16);
        assert_eq!(settings.note_line_height, 1.85);
        assert_eq!(settings.letter_spacing, 0.0);
        assert_eq!(settings.font_preset, super::FontPreset::System);
        assert_eq!(settings.theme_background.surface_opacity, 92);
        assert!(!settings.memory.enabled);
        assert!(!settings.memory.allow_model_write);
        assert_eq!(settings.update_proxy.mode, UpdateProxyMode::System);
        assert!(settings.update_proxy.url.is_empty());
        assert!(settings.knowledge.enabled);
        assert!(!settings.knowledge.auto_retrieve);
        assert_eq!(settings.knowledge.chunk_target_chars, 1_600);
        assert_eq!(settings.knowledge.chunk_max_chars, 2_400);
        assert_eq!(settings.knowledge.chunk_overlap_chars, 200);
        assert_eq!(settings.knowledge.top_k, 8);
        assert_eq!(settings.knowledge.context_max_bytes, 64 * 1024);
        assert_eq!(
            settings.knowledge.retrieval_mode,
            KnowledgeRetrievalMode::Lexical
        );
        assert!(settings.knowledge.mineru_cloud_enabled);
        assert!(!settings.knowledge.embedding_enabled);
        assert!(!settings.knowledge.allow_remote_embedding);
    }

    #[test]
    fn version_sixteen_settings_receive_safe_knowledge_defaults() {
        let value = serde_json::json!({
            "version": 16,
            "interfaceLanguage": "zh",
            "theme": "system",
            "fontSize": 14,
            "noteFontSize": 16,
            "noteLineHeight": 1.85,
            "retryEnabled": true,
            "retryAttempts": 5,
            "maxOutputTokens": 32768
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        let settings = settings.normalize_and_validate().unwrap();
        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
        assert_eq!(settings.knowledge, KnowledgeSettings::default());
    }

    #[test]
    fn knowledge_settings_validate_chunk_retrieval_and_endpoint_limits() {
        let invalid_target = AppSettings {
            knowledge: KnowledgeSettings {
                chunk_target_chars: 255,
                ..KnowledgeSettings::default()
            },
            ..AppSettings::default()
        };
        assert!(invalid_target.normalize_and_validate().is_err());

        let invalid_overlap = AppSettings {
            knowledge: KnowledgeSettings {
                chunk_overlap_chars: 801,
                ..KnowledgeSettings::default()
            },
            ..AppSettings::default()
        };
        assert!(invalid_overlap.normalize_and_validate().is_err());

        let invalid_vector = AppSettings {
            knowledge: KnowledgeSettings {
                retrieval_mode: KnowledgeRetrievalMode::Vector,
                embedding_enabled: false,
                ..KnowledgeSettings::default()
            },
            ..AppSettings::default()
        };
        assert!(invalid_vector.normalize_and_validate().is_err());

        let invalid_endpoint = AppSettings {
            knowledge: KnowledgeSettings {
                mineru_endpoint: "http://localhost:8080/api/v4".to_string(),
                ..KnowledgeSettings::default()
            },
            ..AppSettings::default()
        };
        assert!(invalid_endpoint.normalize_and_validate().is_err());

        let valid = AppSettings {
            knowledge: KnowledgeSettings {
                retrieval_mode: KnowledgeRetrievalMode::Hybrid,
                embedding_enabled: true,
                hybrid_enabled: true,
                mineru_endpoint: "https://example.com/api/v4".to_string(),
                ..KnowledgeSettings::default()
            },
            ..AppSettings::default()
        };
        assert!(valid.normalize_and_validate().is_ok());
    }

    #[test]
    fn agent_rounds_accept_only_documented_presets() {
        for rounds in [5, 10, 20, 50, 100] {
            let settings = AppSettings {
                agent_max_rounds: rounds,
                ..AppSettings::default()
            };
            assert!(settings.normalize_and_validate().is_ok(), "preset {rounds}");
        }

        for rounds in [0, 1, 19, 21, 101] {
            let settings = AppSettings {
                agent_max_rounds: rounds,
                ..AppSettings::default()
            };
            assert!(
                settings.normalize_and_validate().is_err(),
                "invalid {rounds}"
            );
        }
    }

    #[test]
    fn normalizes_user_input_and_rejects_invalid_avatar() {
        let mut settings = AppSettings {
            user_display_name: "  Mnemora  ".to_string(),
            ..AppSettings::default()
        };
        settings = settings.normalize_and_validate().unwrap();
        assert_eq!(settings.user_display_name, "Mnemora");

        settings.user_avatar = "file:///secret".to_string();
        assert!(settings.normalize_and_validate().is_err());
    }

    #[test]
    fn migrates_retry_default_and_accepts_uploaded_avatar() {
        let settings = AppSettings {
            version: 2,
            retry_attempts: 1,
            user_avatar: "data:image/png;base64,iVBORw0KGgo=".to_string(),
            ..AppSettings::default()
        }
        .normalize_and_validate()
        .unwrap();
        assert_eq!(settings.retry_attempts, 5);
        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
    }

    #[test]
    fn accepts_safe_theme_background_and_rejects_external_resources() {
        let mut settings = AppSettings {
            theme_background: super::ThemeBackgroundSettings {
                enabled: true,
                css: "linear-gradient(135deg, #f7f8f6, #dfeae3)".to_string(),
                surface_opacity: 88,
            },
            ..AppSettings::default()
        };
        settings = settings.normalize_and_validate().unwrap();
        assert_eq!(settings.theme_background.surface_opacity, 88);

        settings.theme_background.css = "url(https://example.com/bg.png)".to_string();
        assert!(settings.clone().normalize_and_validate().is_ok());
        settings.theme_background.css = "url(javascript:alert(1))".to_string();
        assert!(settings.clone().normalize_and_validate().is_err());
        settings.theme_background.css = "url(file:///C:/secret.png)".to_string();
        assert!(settings.normalize_and_validate().is_err());
    }

    #[test]
    fn old_settings_without_theme_fields_use_new_defaults() {
        let value = serde_json::json!({
            "version": 5,
            "interfaceLanguage": "zh",
            "theme": "system",
            "themeColor": "neutral",
            "fontSize": 14,
            "retryEnabled": true,
            "retryAttempts": 5,
            "maxOutputTokens": 32768
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.theme_preset, ThemePreset::Mnemora);
        assert!(!settings.theme_background.enabled);
        assert_eq!(settings.theme_background.surface_opacity, 92);
        // 旧配置文件里没有这个键，反序列化必须落到 true 而不是 false，
        // 否则老用户升级后拿不到流式保活。
        assert!(settings.deep_note_stream_keepalive);
    }

    #[test]
    fn version_six_settings_without_typography_fields_use_new_defaults() {
        let value = serde_json::json!({
            "version": 6,
            "interfaceLanguage": "zh",
            "theme": "system",
            "themePreset": "mnemora",
            "themeColor": "neutral",
            "fontSize": 16,
            "retryEnabled": true,
            "retryAttempts": 5,
            "maxOutputTokens": 32768
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.font_size, 16);
        assert_eq!(settings.letter_spacing, 0.0);
        assert_eq!(settings.font_preset, super::FontPreset::System);
        assert_eq!(
            settings.chinese_font_family,
            super::ChineseFontFamily::System
        );
        assert_eq!(settings.latin_font_family, super::LatinFontFamily::System);
    }

    #[test]
    fn version_eleven_settings_receive_note_typography_defaults() {
        let value = serde_json::json!({
            "version": 11,
            "interfaceLanguage": "zh",
            "theme": "system",
            "themePreset": "mnemora",
            "themeColor": "neutral",
            "fontSize": 15,
            "fontPreset": "system",
            "chineseFontFamily": "system",
            "latinFontFamily": "system",
            "retryEnabled": true,
            "retryAttempts": 5,
            "maxOutputTokens": 32768
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        let settings = settings.normalize_and_validate().unwrap();

        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
        assert_eq!(settings.note_font_size, 16);
        assert_eq!(settings.note_line_height, 1.85);
        assert_eq!(settings.note_font_preset, super::FontPreset::System);
        assert_eq!(
            settings.note_chinese_font_family,
            super::ChineseFontFamily::System
        );
        assert_eq!(
            settings.note_latin_font_family,
            super::LatinFontFamily::System
        );
        assert!(settings.show_chat_task_progress);
    }

    #[test]
    fn version_twelve_settings_receive_the_chat_task_panel_default() {
        let value = serde_json::json!({
            "version": 12,
            "interfaceLanguage": "zh",
            "theme": "system",
            "themePreset": "mnemora",
            "themeColor": "neutral",
            "fontSize": 14,
            "noteFontSize": 16,
            "noteLineHeight": 1.85,
            "retryEnabled": true,
            "retryAttempts": 5,
            "maxOutputTokens": 32768
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        let settings = settings.normalize_and_validate().unwrap();

        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
        assert!(settings.show_chat_task_progress);
    }

    #[test]
    fn version_thirteen_settings_receive_safe_pet_defaults() {
        let value = serde_json::json!({
            "version": 13,
            "interfaceLanguage": "zh",
            "theme": "system",
            "themePreset": "mnemora",
            "themeColor": "neutral",
            "fontSize": 14,
            "noteFontSize": 16,
            "noteLineHeight": 1.85,
            "retryEnabled": true,
            "retryAttempts": 5,
            "maxOutputTokens": 32768
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        let settings = settings.normalize_and_validate().unwrap();

        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
        assert!(!settings.pet.enabled);
        assert!(!settings.pet.show_on_startup);
        assert!(settings.pet.task_events);
    }

    #[test]
    fn version_fourteen_settings_receive_the_builtin_pet_selection() {
        let value = serde_json::json!({
            "version": 14,
            "interfaceLanguage": "zh",
            "theme": "system",
            "themePreset": "mnemora",
            "themeColor": "neutral",
            "fontSize": 14,
            "noteFontSize": 16,
            "noteLineHeight": 1.85,
            "retryEnabled": true,
            "retryAttempts": 5,
            "maxOutputTokens": 32768,
            "pet": {
                "enabled": true,
                "size": 176,
                "opacity": 96
            }
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        let settings = settings.normalize_and_validate().unwrap();

        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
        assert_eq!(settings.pet.selected_pet_id, "mimo");
        assert!(settings.pet.enabled);
    }

    #[test]
    fn version_fifteen_settings_receive_the_locked_pet_default() {
        let value = serde_json::json!({
            "version": 15,
            "interfaceLanguage": "zh",
            "theme": "system",
            "fontSize": 14,
            "noteFontSize": 16,
            "noteLineHeight": 1.85,
            "retryEnabled": true,
            "retryAttempts": 5,
            "maxOutputTokens": 32768,
            "pet": {"enabled": true, "locked": false}
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        let settings = settings.normalize_and_validate().unwrap();
        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
        assert!(settings.pet.locked);
    }

    #[test]
    fn rejects_invalid_pet_window_settings() {
        let invalid_size = AppSettings {
            pet: super::PetSettings {
                size: 80,
                ..super::PetSettings::default()
            },
            ..AppSettings::default()
        };
        assert!(invalid_size.normalize_and_validate().is_err());

        let invalid_position = AppSettings {
            pet: super::PetSettings {
                position_x: Some(f64::NAN),
                ..super::PetSettings::default()
            },
            ..AppSettings::default()
        };
        assert!(invalid_position.normalize_and_validate().is_err());

        let invalid_pet_id = AppSettings {
            pet: super::PetSettings {
                selected_pet_id: "../unsafe".to_string(),
                ..super::PetSettings::default()
            },
            ..AppSettings::default()
        };
        assert!(invalid_pet_id.normalize_and_validate().is_err());
    }

    #[test]
    fn rejects_note_typography_outside_supported_ranges() {
        let too_small = AppSettings {
            note_font_size: 11,
            ..AppSettings::default()
        };
        assert!(too_small.normalize_and_validate().is_err());

        let too_loose = AppSettings {
            note_line_height: 2.45,
            ..AppSettings::default()
        };
        assert!(too_loose.normalize_and_validate().is_err());
    }

    #[test]
    fn version_eight_theme_preset_survives_without_resetting_other_preferences() {
        let value = serde_json::json!({
            "version": 8,
            "interfaceLanguage": "en",
            "theme": "dark",
            "themePreset": "forest",
            "themeColor": "violet",
            "themeBackground": {
                "enabled": true,
                "css": "linear-gradient(135deg, #111111, #222222)",
                "surfaceOpacity": 88
            },
            "fontSize": 18,
            "letterSpacing": 0.4,
            "retryEnabled": true,
            "retryAttempts": 4,
            "maxOutputTokens": 65536
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        let settings = settings.normalize_and_validate().unwrap();

        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
        assert_eq!(settings.theme, ThemeMode::Dark);
        assert_eq!(settings.theme_preset, ThemePreset::Forest);
        assert_eq!(settings.theme_color, ThemeColor::Neutral);
        assert!(settings.theme_background.enabled);
        assert_eq!(settings.theme_background.surface_opacity, 88);
        assert_eq!(settings.font_size, 18);
        assert_eq!(settings.letter_spacing, 0.4);
        assert_eq!(settings.retry_attempts, 4);
        assert_eq!(settings.max_output_tokens, 65_536);
    }

    #[test]
    fn normalizes_and_validates_update_proxy() {
        let settings = AppSettings {
            update_proxy: super::UpdateProxySettings {
                mode: UpdateProxyMode::Manual,
                url: " 127.0.0.1:7890 ".to_string(),
            },
            ..AppSettings::default()
        }
        .normalize_and_validate()
        .unwrap();
        assert_eq!(settings.update_proxy.url, "http://127.0.0.1:7890/");

        for url in [
            "socks5://127.0.0.1:7890",
            "http://user:password@127.0.0.1:7890",
            "http://127.0.0.1:7890?token=secret",
        ] {
            let settings = AppSettings {
                update_proxy: super::UpdateProxySettings {
                    mode: UpdateProxyMode::Manual,
                    url: url.to_string(),
                },
                ..AppSettings::default()
            };
            assert!(settings.normalize_and_validate().is_err(), "invalid {url}");
        }
    }
}
