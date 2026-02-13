//! Slash command compilation from skills.

use crate::loader::SkillEntry;

/// A compiled slash command from a user-invocable skill.
#[derive(Debug, Clone)]
pub struct SkillCommand {
    /// Command name (without leading slash).
    pub name: String,
    /// Skill name this command invokes.
    pub skill_name: String,
    /// Description for help display.
    pub description: String,
}

/// Build slash commands from loaded skills.
///
/// Only skills with `user_invocable: true` are included.
pub fn build_skill_commands(skills: &[SkillEntry]) -> Vec<SkillCommand> {
    skills
        .iter()
        .filter(|s| s.user_invocable)
        .map(|s| SkillCommand {
            name: s.name.clone(),
            skill_name: s.name.clone(),
            description: s.description.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{SkillEntry, SkillSource};
    use std::path::PathBuf;

    #[test]
    fn test_build_skill_commands() {
        let skills = vec![
            SkillEntry {
                name: "review-pr".into(),
                description: "Review a PR".into(),
                allowed_tools: vec![],
                user_invocable: true,
                content: String::new(),
                source: SkillSource::Managed,
                file_path: PathBuf::new(),
            },
            SkillEntry {
                name: "internal-only".into(),
                description: "Not user-invocable".into(),
                allowed_tools: vec![],
                user_invocable: false,
                content: String::new(),
                source: SkillSource::Managed,
                file_path: PathBuf::new(),
            },
        ];

        let cmds = build_skill_commands(&skills);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "review-pr");
    }
}
