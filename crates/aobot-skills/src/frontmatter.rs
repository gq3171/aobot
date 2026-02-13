//! YAML frontmatter parser for skill files.

use serde::Deserialize;

/// Parsed skill frontmatter.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SkillFrontmatter {
    /// Skill name (identifier).
    #[serde(default)]
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Tools this skill is allowed to use.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Whether users can invoke this skill as a slash command.
    #[serde(default)]
    pub user_invocable: bool,
}

/// Parse a skill file, separating frontmatter from body.
///
/// Returns `(frontmatter, body)`. If no frontmatter is found,
/// returns default frontmatter and the entire content as body.
pub fn parse_skill_file(content: &str) -> (SkillFrontmatter, String) {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        return (SkillFrontmatter::default(), content.to_string());
    }

    // Find the closing ---
    let after_first = &trimmed[3..];
    if let Some(end_pos) = after_first.find("\n---") {
        let yaml_str = &after_first[..end_pos];
        let body_start = end_pos + 4; // skip \n---
        let body = after_first[body_start..]
            .trim_start_matches('\n')
            .to_string();

        match serde_json::from_value(
            serde_json::to_value(
                yaml_str
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| {
                        let parts: Vec<&str> = l.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            (parts[0].trim(), parts[1].trim())
                        } else {
                            (l.trim(), "")
                        }
                    })
                    .fold(serde_json::Map::new(), |mut map, (key, value)| {
                        let parsed_value = parse_yaml_value(value);
                        map.insert(key.to_string(), parsed_value);
                        map
                    }),
            )
            .unwrap_or_default(),
        ) {
            Ok(fm) => (fm, body),
            Err(_) => (SkillFrontmatter::default(), content.to_string()),
        }
    } else {
        (SkillFrontmatter::default(), content.to_string())
    }
}

/// Simple YAML value parser for frontmatter fields.
fn parse_yaml_value(value: &str) -> serde_json::Value {
    let trimmed = value.trim();

    // Boolean
    if trimmed == "true" {
        return serde_json::Value::Bool(true);
    }
    if trimmed == "false" {
        return serde_json::Value::Bool(false);
    }

    // Array: [item1, item2]
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let items: Vec<serde_json::Value> = inner
            .split(',')
            .map(|s| serde_json::Value::String(s.trim().to_string()))
            .collect();
        return serde_json::Value::Array(items);
    }

    // Number
    if let Ok(n) = trimmed.parse::<i64>() {
        return serde_json::Value::Number(n.into());
    }

    // String (remove surrounding quotes if present)
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed);
    serde_json::Value::String(unquoted.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_file_with_frontmatter() {
        let content = r#"---
name: review-pr
description: Review a GitHub pull request
allowed_tools: [bash, read, grep, find]
user_invocable: true
---

# Review PR

Analyze the PR changes and provide feedback.
"#;
        let (fm, body) = parse_skill_file(content);
        assert_eq!(fm.name, "review-pr");
        assert_eq!(fm.description, "Review a GitHub pull request");
        assert_eq!(fm.allowed_tools, vec!["bash", "read", "grep", "find"]);
        assert!(fm.user_invocable);
        assert!(body.contains("# Review PR"));
    }

    #[test]
    fn test_parse_skill_file_without_frontmatter() {
        let content = "# Just some markdown\n\nNo frontmatter here.";
        let (fm, body) = parse_skill_file(content);
        assert_eq!(fm.name, "");
        assert!(body.contains("Just some markdown"));
    }
}
