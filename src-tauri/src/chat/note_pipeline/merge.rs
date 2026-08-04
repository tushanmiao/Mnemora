use super::types::{NotePatch, NotePatchAction};

fn h2_ranges(markdown: &str) -> Vec<(String, usize, usize)> {
    let mut starts = Vec::new();
    let mut offset = 0usize;
    for line in markdown.split_inclusive('\n') {
        if let Some(heading) = line.strip_prefix("## ") {
            starts.push((heading.trim().to_string(), offset));
        }
        offset += line.len();
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, (heading, start))| {
            let end = starts
                .get(index + 1)
                .map_or(markdown.len(), |(_, next)| *next);
            (heading.clone(), *start, end)
        })
        .collect()
}

pub fn apply_note_patches(
    original: &str,
    patches: &[NotePatch],
) -> Result<(String, Vec<String>), String> {
    if patches.is_empty() {
        return Err("模型没有返回可应用的笔记补丁。".to_string());
    }
    let mut content = original.trim().to_string();
    let mut warnings = Vec::new();
    for patch in patches {
        let markdown = patch.markdown.trim();
        if markdown.is_empty() || !markdown.starts_with("## ") {
            warnings.push(format!("章节“{}”的补丁格式无效，已跳过。", patch.heading));
            continue;
        }
        match patch.action {
            NotePatchAction::AddSection => {
                content.push_str("\n\n");
                content.push_str(markdown);
            }
            NotePatchAction::AppendToSection => {
                let target = patch.target_heading.trim();
                let range = h2_ranges(&content)
                    .into_iter()
                    .find(|(heading, _, _)| heading.eq_ignore_ascii_case(target));
                if let Some((_, _, end)) = range {
                    let addition = markdown
                        .lines()
                        .skip(1)
                        .collect::<Vec<_>>()
                        .join("\n")
                        .trim()
                        .to_string();
                    if addition.is_empty() {
                        warnings.push(format!("章节“{}”没有可追加内容。", patch.heading));
                        continue;
                    }
                    let insert_at = content[..end].trim_end().len();
                    content.insert_str(insert_at, &format!("\n\n{addition}"));
                } else {
                    warnings.push(format!("未找到目标章节“{target}”，已按新增章节处理。"));
                    content.push_str("\n\n");
                    content.push_str(markdown);
                }
            }
            NotePatchAction::ReplaceSection => {
                let target = patch.target_heading.trim();
                let range = h2_ranges(&content)
                    .into_iter()
                    .find(|(heading, _, _)| heading.eq_ignore_ascii_case(target));
                if let Some((_, start, end)) = range {
                    content.replace_range(start..end, &format!("{}\n", markdown));
                } else {
                    warnings.push(format!("未找到待修正章节“{target}”，原内容保持不变。"));
                }
            }
        }
    }
    if content == original.trim() {
        return Err("没有任何补丁成功应用，原笔记保持不变。".to_string());
    }
    Ok((content, warnings))
}

pub fn compact_diff(old: &str, new: &str) -> String {
    let old_lines = old.lines().collect::<Vec<_>>();
    let new_lines = new.lines().collect::<Vec<_>>();
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let old_end = old_lines.len().saturating_sub(suffix);
    let new_end = new_lines.len().saturating_sub(suffix);
    let context_start = prefix.saturating_sub(3);
    let mut output = vec!["--- 旧笔记".to_string(), "+++ 新笔记".to_string()];
    for line in &old_lines[context_start..prefix] {
        output.push(format!(" {line}"));
    }
    for line in &old_lines[prefix..old_end] {
        output.push(format!("-{line}"));
    }
    for line in &new_lines[prefix..new_end] {
        output.push(format!("+{line}"));
    }
    let context_end = (new_end + 3).min(new_lines.len());
    for line in &new_lines[new_end..context_end] {
        output.push(format!(" {line}"));
    }
    output.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{apply_note_patches, compact_diff};
    use crate::chat::note_pipeline::types::{NotePatch, NotePatchAction};

    #[test]
    fn applies_append_and_preserves_other_sections() {
        let original = "# T\n\n## A\n\nold\n\n## B\n\nkeep";
        let patch = NotePatch {
            action: NotePatchAction::AppendToSection,
            target_heading: "A".to_string(),
            heading: "A".to_string(),
            markdown: "## A\n\nnew".to_string(),
            needs_supplement: false,
            source_message_ids: vec![],
        };
        let (updated, warnings) = apply_note_patches(original, &[patch]).unwrap();
        assert!(warnings.is_empty());
        assert!(updated.contains("old\n\nnew"));
        assert!(updated.contains("## B\n\nkeep"));
        assert!(compact_diff(original, &updated).contains("+new"));
    }

    #[test]
    fn missing_replace_target_never_overwrites_original() {
        let original = "# T\n\n## A\n\nold";
        let patch = NotePatch {
            action: NotePatchAction::ReplaceSection,
            target_heading: "missing".to_string(),
            heading: "missing".to_string(),
            markdown: "## missing\n\nnew".to_string(),
            needs_supplement: false,
            source_message_ids: vec![],
        };
        assert!(apply_note_patches(original, &[patch]).is_err());
    }
}
