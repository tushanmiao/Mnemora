use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::library::types::{NoteEditProposal, NotePipelinePhase, NotePipelineRun};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteSectionKind {
    Prerequisite,
    Concept,
    Comparison,
    Pitfall,
    Example,
    Summary,
    Selfcheck,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteSection {
    pub id: String,
    pub heading: String,
    pub kind: DeepNoteSectionKind,
    pub brief: String,
    #[serde(default)]
    pub needs_supplement: bool,
    #[serde(default)]
    pub source_message_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteOutline {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub weak_points: Vec<String>,
    pub sections: Vec<DeepNoteSection>,
}

impl DeepNoteOutline {
    pub fn validate(mut self, valid_message_ids: &HashSet<String>) -> Result<Self, String> {
        self.title = self.title.trim().trim_start_matches('#').trim().to_string();
        if self.title.is_empty() || self.title.chars().count() > 500 {
            return Err("深度笔记标题为空或过长。".to_string());
        }
        self.summary = self.summary.trim().to_string();
        if self.sections.is_empty() || self.sections.len() > 40 {
            return Err("深度笔记提纲必须包含 1 到 40 个章节。".to_string());
        }
        let mut ids = HashSet::new();
        for section in &mut self.sections {
            section.id = section.id.trim().to_string();
            section.heading = section
                .heading
                .trim()
                .trim_start_matches('#')
                .trim()
                .to_string();
            section.brief = section.brief.trim().to_string();
            if section.id.is_empty() || section.heading.is_empty() || section.brief.is_empty() {
                return Err("深度笔记章节缺少 id、heading 或 brief。".to_string());
            }
            if !ids.insert(section.id.clone()) {
                return Err(format!("深度笔记提纲包含重复章节 ID：{}。", section.id));
            }
            section
                .source_message_ids
                .retain(|message_id| valid_message_ids.contains(message_id));
            section.source_message_ids.sort();
            section.source_message_ids.dedup();
        }
        Ok(self)
    }

    pub fn select(&self, selected: &HashSet<String>) -> Result<Self, String> {
        let mut outline = self.clone();
        outline
            .sections
            .retain(|section| selected.contains(&section.id));
        if outline.sections.is_empty() {
            return Err("请至少保留一个章节。".to_string());
        }
        Ok(outline)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePipelineStartRequest {
    pub conversation_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePipelineConfirmRequest {
    pub run_id: String,
    pub selected_section_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePipelineAdjustRequest {
    pub run_id: String,
    pub requirement: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NotePipelineProgress {
    Progress {
        run_id: String,
        phase: NotePipelinePhase,
        current: Option<usize>,
        total: Option<usize>,
        message: String,
    },
    OutlineReady {
        run: NotePipelineRun,
    },
    Done {
        run: NotePipelineRun,
        degraded: bool,
    },
    Cancelled {
        run: NotePipelineRun,
    },
    Error {
        run_id: String,
        message: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEditPrepareRequest {
    pub note_id: String,
    pub conversation_id: String,
    #[serde(default)]
    pub selected_text: String,
    #[serde(default)]
    pub section_heading: String,
    #[serde(default)]
    pub requirement: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NotePatchAction {
    AddSection,
    AppendToSection,
    ReplaceSection,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMergePlanItem {
    pub action: NotePatchAction,
    #[serde(default)]
    pub target_heading: String,
    pub heading: String,
    pub brief: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMergePlan {
    #[serde(default)]
    pub title: String,
    pub operations: Vec<NoteMergePlanItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePatch {
    pub action: NotePatchAction,
    #[serde(default)]
    pub target_heading: String,
    pub heading: String,
    pub markdown: String,
    #[serde(default)]
    pub needs_supplement: bool,
    #[serde(default)]
    pub source_message_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePatchSet {
    #[serde(default)]
    pub title: String,
    pub patches: Vec<NotePatch>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEditPrepareResult {
    pub proposal: NoteEditProposal,
    pub warnings: Vec<String>,
}
