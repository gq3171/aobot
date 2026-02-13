//! Tool group definitions.
//!
//! Groups provide convenient shorthands for sets of related tools.
//! Referenced with `group:name` syntax in configuration.

use std::collections::HashMap;

use once_cell::sync::Lazy;

/// All built-in tool group definitions.
///
/// Keys are group names (without `group:` prefix), values are tool name slices.
pub static TOOL_GROUPS: Lazy<HashMap<&'static str, &'static [&'static str]>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("fs", &["read", "write", "edit"][..]);
    m.insert("runtime", &["bash", "process"][..]);
    m.insert("web", &["web_search", "web_fetch"][..]);
    m.insert("memory", &["memory_search", "memory_get"][..]);
    m.insert(
        "sessions",
        &[
            "sessions_list",
            "sessions_history",
            "sessions_send",
            "sessions_spawn",
            "session_status",
        ][..],
    );
    m.insert("messaging", &["message"][..]);
    m.insert("search", &["grep", "find", "ls"][..]);
    m.insert("media", &["image", "tts"][..]);
    m.insert("automation", &["cron", "gateway"][..]);
    m
});

/// Expand a single name that may be a `group:xxx` reference.
///
/// If the name starts with `group:` and the group exists, returns the
/// individual tool names from that group. Otherwise returns the name as-is.
pub fn expand_name(name: &str) -> Vec<String> {
    if let Some(group_name) = name.strip_prefix("group:") {
        if let Some(tools) = TOOL_GROUPS.get(group_name) {
            return tools.iter().map(|s| s.to_string()).collect();
        }
    }
    vec![name.to_string()]
}

/// Expand a list of names, resolving any `group:xxx` references.
pub fn expand_names(names: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for name in names {
        result.extend(expand_name(name));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_group() {
        let expanded = expand_name("group:fs");
        assert_eq!(expanded, vec!["read", "write", "edit"]);
    }

    #[test]
    fn test_expand_unknown_group() {
        let expanded = expand_name("group:nonexistent");
        assert_eq!(expanded, vec!["group:nonexistent"]);
    }

    #[test]
    fn test_expand_plain_name() {
        let expanded = expand_name("bash");
        assert_eq!(expanded, vec!["bash"]);
    }

    #[test]
    fn test_expand_names_mixed() {
        let names = vec![
            "group:fs".to_string(),
            "bash".to_string(),
            "group:web".to_string(),
        ];
        let expanded = expand_names(&names);
        assert_eq!(
            expanded,
            vec!["read", "write", "edit", "bash", "web_search", "web_fetch"]
        );
    }
}
