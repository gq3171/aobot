//! Skill file discovery and loading.

use std::path::{Path, PathBuf};

use crate::frontmatter::parse_skill_file;

/// Source of a skill definition.
#[derive(Debug, Clone, PartialEq)]
pub enum SkillSource {
    /// Built-in skill shipped with aobot.
    Bundled,
    /// User-managed global skill (~/.aobot/skills/).
    Managed,
    /// Project-local skill (./.aobot/skills/).
    Workspace,
}

/// A loaded skill entry.
#[derive(Debug, Clone)]
pub struct SkillEntry {
    /// Skill name (identifier).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Tools this skill is allowed to use.
    pub allowed_tools: Vec<String>,
    /// Whether users can invoke this skill as a slash command.
    pub user_invocable: bool,
    /// Markdown body (injected as system prompt).
    pub content: String,
    /// Source of this skill.
    pub source: SkillSource,
    /// File path of the skill definition.
    pub file_path: PathBuf,
}

/// Load skills from multiple directories.
///
/// Later directories have higher priority — if a skill name appears in
/// multiple directories, the later one wins.
///
/// Directory priority (low → high):
/// 1. Bundled skills
/// 2. Global skills (`~/.aobot/skills/`)
/// 3. Workspace skills (`./.aobot/skills/`)
pub fn load_skills(dirs: &[(PathBuf, SkillSource)]) -> Vec<SkillEntry> {
    let mut skills_map = std::collections::HashMap::new();

    for (dir, source) in dirs {
        if !dir.exists() {
            continue;
        }

        let entries = discover_skill_files(dir);
        for file_path in entries {
            match load_skill_file(&file_path, source.clone()) {
                Ok(entry) => {
                    tracing::debug!(
                        skill = %entry.name,
                        source = ?source,
                        "Loaded skill"
                    );
                    skills_map.insert(entry.name.clone(), entry);
                }
                Err(e) => {
                    tracing::warn!(
                        path = %file_path.display(),
                        "Failed to load skill: {e}"
                    );
                }
            }
        }
    }

    skills_map.into_values().collect()
}

/// Discover SKILL.md files in a directory.
fn discover_skill_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if dir.is_file() && is_skill_file(dir) {
        files.push(dir.to_path_buf());
        return files;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return files,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Look for SKILL.md inside the subdirectory
            let skill_file = path.join("SKILL.md");
            if skill_file.exists() {
                files.push(skill_file);
            }
        } else if is_skill_file(&path) {
            files.push(path);
        }
    }

    files
}

fn is_skill_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "SKILL.md" || n.ends_with(".skill.md"))
}

/// Load a single skill file.
fn load_skill_file(path: &Path, source: SkillSource) -> anyhow::Result<SkillEntry> {
    let content = std::fs::read_to_string(path)?;
    let (fm, body) = parse_skill_file(&content);

    // Use directory name as fallback for skill name
    let name = if fm.name.is_empty() {
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string()
    } else {
        fm.name
    };

    Ok(SkillEntry {
        name,
        description: fm.description,
        allowed_tools: fm.allowed_tools,
        user_invocable: fm.user_invocable,
        content: body,
        source,
        file_path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_skill_file() {
        assert!(is_skill_file(Path::new("/foo/bar/SKILL.md")));
        assert!(is_skill_file(Path::new("/foo/review.skill.md")));
        assert!(!is_skill_file(Path::new("/foo/README.md")));
    }
}
