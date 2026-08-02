//! 规范化前端提交的全局提示词。
//!
//! 默认内容和用户修改都由设置页管理；后端不再额外注入不可见的提示词。

pub fn prepend_core_system_prompt(system_prompt: &str) -> String {
    // 全局提示词已由设置页完整提供；后端不再隐式重复注入默认内容。
    system_prompt.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::prepend_core_system_prompt;

    #[test]
    fn keeps_user_prompt_without_hidden_rules() {
        let prompt = prepend_core_system_prompt("用户规则");
        assert_eq!(prompt, "用户规则");
    }
}
