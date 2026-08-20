use std::collections::{HashMap, HashSet, VecDeque};

use super::types::{DeepNoteDagNode, DeepNoteNodeStatus, DeepNoteNodeType, DeepNoteSection};

#[derive(Debug, Clone)]
pub struct DeepNoteDagScheduler {
    nodes: Vec<DeepNoteDagNode>,
}

impl DeepNoteDagScheduler {
    pub fn new(nodes: Vec<DeepNoteDagNode>) -> Result<Self, String> {
        let ids = nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<HashSet<_>>();
        if ids.len() != nodes.len() {
            return Err("深度笔记执行图包含重复节点 ID。".to_string());
        }
        if nodes
            .iter()
            .flat_map(|node| node.depends_on.iter())
            .any(|dependency| !ids.contains(dependency.as_str()))
        {
            return Err("深度笔记执行图包含悬空依赖。".to_string());
        }
        Ok(Self { nodes })
    }

    pub fn nodes(&self) -> &[DeepNoteDagNode] {
        &self.nodes
    }

    pub fn node(&self, node_id: &str) -> Option<&DeepNoteDagNode> {
        self.nodes.iter().find(|node| node.node_id == node_id)
    }

    pub fn node_mut(&mut self, node_id: &str) -> Result<&mut DeepNoteDagNode, String> {
        self.nodes
            .iter_mut()
            .find(|node| node.node_id == node_id)
            .ok_or_else(|| format!("深度笔记执行节点不存在：{node_id}"))
    }

    pub fn transition(&mut self, node_id: &str, next: DeepNoteNodeStatus) -> Result<(), String> {
        let node = self.node_mut(node_id)?;
        if node.status == next {
            return Ok(());
        }
        if !valid_transition(node.status, next) {
            return Err(format!(
                "深度笔记执行节点 {} 不能从 {} 转换为 {}。",
                node.node_id,
                node.status.as_str(),
                next.as_str()
            ));
        }
        node.status = next;
        Ok(())
    }

    #[cfg(test)]
    pub fn force_status(
        &mut self,
        node_id: &str,
        status: DeepNoteNodeStatus,
    ) -> Result<(), String> {
        self.node_mut(node_id)?.status = status;
        Ok(())
    }

    pub fn complete_preparation(&mut self) {
        for node in &mut self.nodes {
            if matches!(
                node.node_type,
                DeepNoteNodeType::AnalyzeInput
                    | DeepNoteNodeType::ReconSource
                    | DeepNoteNodeType::ExtractEvidence
                    | DeepNoteNodeType::BuildLedger
            ) {
                node.status = DeepNoteNodeStatus::Completed;
                if node.output_ref.is_none() {
                    node.output_ref = Some(match node.node_type {
                        DeepNoteNodeType::AnalyzeInput => "input-snapshot".to_string(),
                        DeepNoteNodeType::ReconSource => "source-reconciliation".to_string(),
                        DeepNoteNodeType::ExtractEvidence => node
                            .section_id
                            .as_ref()
                            .map(|section_id| format!("evidence:{section_id}"))
                            .unwrap_or_else(|| "evidence".to_string()),
                        DeepNoteNodeType::BuildLedger => "knowledge-ledger".to_string(),
                        _ => unreachable!(),
                    });
                }
            }
        }
        self.refresh_ready();
    }

    pub fn reconcile_completed_section(&mut self, section_id: &str) -> Result<(), String> {
        for prefix in ["draft", "validate"] {
            let node_id = format!("{prefix}:{section_id}");
            let node = self.node_mut(&node_id)?;
            node.status = DeepNoteNodeStatus::Completed;
            if node.output_ref.is_none() {
                node.output_ref = Some(format!("section:{section_id}"));
            }
        }
        self.refresh_ready();
        Ok(())
    }

    pub fn ready_section_ids(&self, limit: usize) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|node| {
                node.node_type == DeepNoteNodeType::DraftSection
                    && node.status == DeepNoteNodeStatus::Ready
            })
            .filter_map(|node| node.section_id.clone())
            .take(limit.max(1))
            .collect()
    }

    pub fn has_unfinished_sections(&self) -> bool {
        self.nodes.iter().any(|node| {
            matches!(
                node.node_type,
                DeepNoteNodeType::DraftSection | DeepNoteNodeType::ValidateSection
            ) && !matches!(
                node.status,
                DeepNoteNodeStatus::Completed
                    | DeepNoteNodeStatus::Failed
                    | DeepNoteNodeStatus::Blocked
                    | DeepNoteNodeStatus::Skipped
            )
        })
    }

    pub fn has_section_failures(&self) -> bool {
        self.nodes.iter().any(|node| {
            matches!(
                node.node_type,
                DeepNoteNodeType::DraftSection | DeepNoteNodeType::ValidateSection
            ) && matches!(
                node.status,
                DeepNoteNodeStatus::Failed | DeepNoteNodeStatus::Blocked
            )
        })
    }

    pub fn skip_unfinished_sections(&mut self) {
        for node in &mut self.nodes {
            if matches!(
                node.node_type,
                DeepNoteNodeType::DraftSection | DeepNoteNodeType::ValidateSection
            ) && !matches!(
                node.status,
                DeepNoteNodeStatus::Completed
                    | DeepNoteNodeStatus::Failed
                    | DeepNoteNodeStatus::Interrupted
            ) {
                node.status = DeepNoteNodeStatus::Skipped;
                node.error_message = Some("任务停止，节点未执行。".to_string());
            }
        }
        self.refresh_ready();
    }

    pub fn interrupt_running(&mut self) {
        for node in &mut self.nodes {
            if node.status == DeepNoteNodeStatus::InProgress {
                node.status = DeepNoteNodeStatus::Interrupted;
                node.error_message = Some("执行被暂停或停止。".to_string());
            }
        }
        self.refresh_ready();
    }

    pub fn prepare_for_resume(&mut self) {
        for node in &mut self.nodes {
            if matches!(
                node.status,
                DeepNoteNodeStatus::InProgress | DeepNoteNodeStatus::Interrupted
            ) {
                node.status = DeepNoteNodeStatus::Pending;
                node.error_message = None;
            }
        }
        self.refresh_ready();
    }

    pub fn refresh_ready(&mut self) {
        loop {
            let statuses = self
                .nodes
                .iter()
                .map(|node| (node.node_id.clone(), node.status))
                .collect::<HashMap<_, _>>();
            let mut changed = false;
            for node in &mut self.nodes {
                if node.status != DeepNoteNodeStatus::Pending {
                    continue;
                }
                let dependency_statuses = node
                    .depends_on
                    .iter()
                    .filter_map(|dependency| statuses.get(dependency).copied())
                    .collect::<Vec<_>>();
                if dependency_statuses.iter().any(|status| {
                    matches!(
                        status,
                        DeepNoteNodeStatus::Failed | DeepNoteNodeStatus::Blocked
                    )
                }) {
                    node.status = DeepNoteNodeStatus::Blocked;
                    node.error_message = Some("前置执行节点失败。".to_string());
                    changed = true;
                } else if dependency_statuses.iter().all(|status| {
                    matches!(
                        status,
                        DeepNoteNodeStatus::Completed | DeepNoteNodeStatus::Skipped
                    )
                }) {
                    node.status = DeepNoteNodeStatus::Ready;
                    node.error_message = None;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
}

pub fn stable_topological_sections(sections: &[DeepNoteSection]) -> Result<Vec<String>, String> {
    let positions = sections
        .iter()
        .enumerate()
        .map(|(position, section)| (section.id.as_str(), position))
        .collect::<HashMap<_, _>>();
    let mut indegree = sections
        .iter()
        .map(|section| (section.id.clone(), section.depends_on.len()))
        .collect::<HashMap<_, _>>();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for section in sections {
        for dependency in &section.depends_on {
            if !positions.contains_key(dependency.as_str()) {
                return Err(format!(
                    "章节“{}”依赖了未选择的章节 {dependency}。",
                    section.heading
                ));
            }
            dependents
                .entry(dependency.as_str())
                .or_default()
                .push(section.id.as_str());
        }
    }
    let mut ready = sections
        .iter()
        .filter(|section| section.depends_on.is_empty())
        .map(|section| section.id.clone())
        .collect::<Vec<_>>();
    ready.sort_by_key(|id| positions.get(id.as_str()).copied().unwrap_or(usize::MAX));
    let mut ready = VecDeque::from(ready);
    let mut ordered = Vec::with_capacity(sections.len());
    while let Some(id) = ready.pop_front() {
        ordered.push(id.clone());
        let mut newly_ready = Vec::new();
        for dependent in dependents.get(id.as_str()).into_iter().flatten() {
            if let Some(count) = indegree.get_mut(*dependent) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    newly_ready.push((*dependent).to_string());
                }
            }
        }
        newly_ready.sort_by_key(|candidate| {
            positions
                .get(candidate.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
        ready.extend(newly_ready);
    }
    if ordered.len() != sections.len() {
        return Err("深度笔记章节依赖存在循环。".to_string());
    }
    Ok(ordered)
}

fn valid_transition(current: DeepNoteNodeStatus, next: DeepNoteNodeStatus) -> bool {
    use DeepNoteNodeStatus as Status;
    matches!(
        (current, next),
        (
            Status::Pending,
            Status::Ready | Status::Blocked | Status::Skipped
        ) | (
            Status::Ready,
            Status::InProgress | Status::Skipped | Status::Interrupted
        ) | (
            Status::InProgress,
            Status::Completed
                | Status::NeedsReview
                | Status::NeedsRevision
                | Status::Failed
                | Status::Interrupted
        ) | (
            Status::NeedsReview,
            Status::InProgress | Status::Failed | Status::Skipped
        ) | (
            Status::NeedsRevision,
            Status::InProgress | Status::Failed | Status::Skipped
        ) | (Status::Failed, Status::Pending)
            | (Status::Blocked, Status::Pending | Status::Skipped)
            | (Status::Interrupted, Status::Pending | Status::Skipped)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::note_pipeline::types::{DeepNoteSectionKind, DeepNoteValidationReport};

    fn node(
        id: &str,
        node_type: DeepNoteNodeType,
        section_id: Option<&str>,
        depends_on: &[&str],
    ) -> DeepNoteDagNode {
        DeepNoteDagNode {
            node_id: id.to_string(),
            node_type,
            section_id: section_id.map(str::to_string),
            depends_on: depends_on
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            status: if depends_on.is_empty() {
                DeepNoteNodeStatus::Ready
            } else {
                DeepNoteNodeStatus::Pending
            },
            attempt_count: 0,
            evidence_ids: Vec::new(),
            input_hash: String::new(),
            output_ref: None,
            validation_json: serde_json::to_string(&DeepNoteValidationReport {
                passed: true,
                errors: Vec::new(),
                warnings: Vec::new(),
                checked_evidence_ids: Vec::new(),
                criteria_coverage: Vec::new(),
            })
            .unwrap(),
            error_message: None,
        }
    }

    fn section(id: &str, depends_on: &[&str]) -> DeepNoteSection {
        DeepNoteSection {
            id: id.to_string(),
            heading: id.to_string(),
            kind: DeepNoteSectionKind::Concept,
            brief: id.to_string(),
            purpose: id.to_string(),
            depends_on: depends_on
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            evidence_requirements: Vec::new(),
            success_criteria: vec!["完成".to_string()],
            source_scope: Vec::new(),
            target_depth: "standard".to_string(),
            allow_ai_supplement: false,
            needs_supplement: false,
            source_message_ids: Vec::new(),
        }
    }

    #[test]
    fn scheduler_releases_only_dependency_ready_sections() {
        let mut scheduler = DeepNoteDagScheduler::new(vec![
            node("ledger", DeepNoteNodeType::BuildLedger, None, &[]),
            node(
                "draft:a",
                DeepNoteNodeType::DraftSection,
                Some("a"),
                &["ledger"],
            ),
            node(
                "validate:a",
                DeepNoteNodeType::ValidateSection,
                Some("a"),
                &["draft:a"],
            ),
            node(
                "draft:b",
                DeepNoteNodeType::DraftSection,
                Some("b"),
                &["validate:a"],
            ),
            node(
                "validate:b",
                DeepNoteNodeType::ValidateSection,
                Some("b"),
                &["draft:b"],
            ),
            node(
                "draft:c",
                DeepNoteNodeType::DraftSection,
                Some("c"),
                &["ledger"],
            ),
        ])
        .unwrap();
        scheduler
            .force_status("ledger", DeepNoteNodeStatus::Completed)
            .unwrap();
        scheduler.refresh_ready();
        assert_eq!(scheduler.ready_section_ids(2), vec!["a", "c"]);
        scheduler
            .transition("draft:a", DeepNoteNodeStatus::InProgress)
            .unwrap();
        scheduler
            .transition("draft:a", DeepNoteNodeStatus::Completed)
            .unwrap();
        scheduler.refresh_ready();
        scheduler
            .transition("validate:a", DeepNoteNodeStatus::InProgress)
            .unwrap();
        scheduler
            .transition("validate:a", DeepNoteNodeStatus::Completed)
            .unwrap();
        scheduler.refresh_ready();
        assert!(scheduler.ready_section_ids(3).contains(&"b".to_string()));
    }

    #[test]
    fn scheduler_blocks_dependents_but_keeps_independent_branch_ready() {
        let mut scheduler = DeepNoteDagScheduler::new(vec![
            node("draft:a", DeepNoteNodeType::DraftSection, Some("a"), &[]),
            node(
                "validate:a",
                DeepNoteNodeType::ValidateSection,
                Some("a"),
                &["draft:a"],
            ),
            node(
                "draft:b",
                DeepNoteNodeType::DraftSection,
                Some("b"),
                &["validate:a"],
            ),
            node("draft:c", DeepNoteNodeType::DraftSection, Some("c"), &[]),
        ])
        .unwrap();
        scheduler
            .transition("draft:a", DeepNoteNodeStatus::InProgress)
            .unwrap();
        scheduler
            .transition("draft:a", DeepNoteNodeStatus::Failed)
            .unwrap();
        scheduler.refresh_ready();
        assert_eq!(
            scheduler.node("validate:a").unwrap().status,
            DeepNoteNodeStatus::Blocked
        );
        scheduler.refresh_ready();
        assert_eq!(
            scheduler.node("draft:b").unwrap().status,
            DeepNoteNodeStatus::Blocked
        );
        assert!(scheduler.ready_section_ids(2).contains(&"c".to_string()));
    }

    #[test]
    fn stable_topology_preserves_plan_order_between_independent_sections() {
        let sections = vec![
            section("later", &["base"]),
            section("independent", &[]),
            section("base", &[]),
        ];
        assert_eq!(
            stable_topological_sections(&sections).unwrap(),
            vec!["independent", "base", "later"]
        );
    }
}
