//! Tool policy system for resolving which tools an agent may use.
//!
//! Resolution chain: `profile → allow/also_allow/deny → by_provider → expand groups`
//!
//! Deny always takes priority over allow.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::groups;

/// Pre-defined tool profiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolProfile {
    /// Only `session_status` — safe for read-only agents.
    Minimal,
    /// File system, runtime, sessions, memory, and image tools.
    Coding,
    /// Messaging, sessions_list, sessions_history, sessions_send, session_status.
    Messaging,
    /// All tools allowed (default).
    #[default]
    Full,
}

impl ToolProfile {
    /// Returns the base tool set for this profile (as group/tool names).
    pub fn base_tools(&self) -> Vec<String> {
        match self {
            ToolProfile::Minimal => vec!["session_status".into()],
            ToolProfile::Coding => vec![
                "group:fs".into(),
                "group:runtime".into(),
                "group:search".into(),
                "group:sessions".into(),
                "group:memory".into(),
                "image".into(),
            ],
            ToolProfile::Messaging => vec![
                "group:messaging".into(),
                "sessions_list".into(),
                "sessions_history".into(),
                "sessions_send".into(),
                "session_status".into(),
            ],
            ToolProfile::Full => vec![], // empty = all tools allowed
        }
    }
}

/// Per-provider tool policy override.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolPolicyOverride {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Tool policy configuration for an agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolPolicy {
    /// Base profile. Defaults to `Full`.
    #[serde(default)]
    pub profile: ToolProfile,
    /// Explicit allow list (replaces profile base when non-empty for non-Full profiles).
    #[serde(default)]
    pub allow: Vec<String>,
    /// Additional tools to allow on top of profile/allow.
    #[serde(default)]
    pub also_allow: Vec<String>,
    /// Tools to deny (takes priority over everything).
    #[serde(default)]
    pub deny: Vec<String>,
    /// Per-provider overrides.
    #[serde(default)]
    pub by_provider: HashMap<String, ToolPolicyOverride>,
}

/// Resolve the effective tool set given a policy and the universe of available tool names.
///
/// Steps:
/// 1. Start with profile base (or all tools if Full).
/// 2. If `allow` is non-empty AND profile is not Full, use `allow` instead of profile base.
/// 3. Add `also_allow`.
/// 4. Expand all `group:xxx` references.
/// 5. Intersect with the available tool universe.
/// 6. Remove `deny` (expanded).
pub fn resolve_effective_tools(policy: &ToolPolicy, all_tool_names: &[String]) -> Vec<String> {
    let all_set: HashSet<&str> = all_tool_names.iter().map(|s| s.as_str()).collect();

    // Step 1+2: determine base set
    let base = if policy.profile == ToolProfile::Full && policy.allow.is_empty() {
        // Full profile with no explicit allow → all tools
        all_tool_names.to_vec()
    } else if !policy.allow.is_empty() {
        // Explicit allow overrides profile base
        groups::expand_names(&policy.allow)
    } else {
        groups::expand_names(&policy.profile.base_tools())
    };

    // Step 3: add also_allow
    let mut allowed: HashSet<String> = base.into_iter().collect();
    for name in groups::expand_names(&policy.also_allow) {
        allowed.insert(name);
    }

    // Step 4+5: intersect with available tools
    let mut result: Vec<String> = allowed
        .into_iter()
        .filter(|name| all_set.contains(name.as_str()))
        .collect();

    // Step 6: remove denied tools
    let denied: HashSet<String> = groups::expand_names(&policy.deny).into_iter().collect();
    result.retain(|name| !denied.contains(name));

    result.sort();
    result
}

/// Check if a single tool is allowed by the policy.
pub fn is_tool_allowed(tool_name: &str, policy: &ToolPolicy, all_tool_names: &[String]) -> bool {
    let effective = resolve_effective_tools(policy, all_tool_names);
    effective.contains(&tool_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_tools() -> Vec<String> {
        vec![
            "read",
            "write",
            "edit",
            "bash",
            "process",
            "grep",
            "find",
            "ls",
            "web_search",
            "web_fetch",
            "memory_search",
            "memory_get",
            "sessions_list",
            "sessions_history",
            "sessions_send",
            "sessions_spawn",
            "session_status",
            "message",
            "image",
            "tts",
            "cron",
            "gateway",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    #[test]
    fn test_full_profile_allows_all() {
        let policy = ToolPolicy::default();
        let effective = resolve_effective_tools(&policy, &all_tools());
        assert_eq!(effective.len(), all_tools().len());
    }

    #[test]
    fn test_minimal_profile() {
        let policy = ToolPolicy {
            profile: ToolProfile::Minimal,
            ..Default::default()
        };
        let effective = resolve_effective_tools(&policy, &all_tools());
        assert_eq!(effective, vec!["session_status"]);
    }

    #[test]
    fn test_coding_profile() {
        let policy = ToolPolicy {
            profile: ToolProfile::Coding,
            ..Default::default()
        };
        let effective = resolve_effective_tools(&policy, &all_tools());
        assert!(effective.contains(&"read".to_string()));
        assert!(effective.contains(&"bash".to_string()));
        assert!(effective.contains(&"image".to_string()));
        assert!(!effective.contains(&"message".to_string()));
    }

    #[test]
    fn test_deny_overrides_allow() {
        let policy = ToolPolicy {
            profile: ToolProfile::Full,
            deny: vec!["bash".into(), "group:web".into()],
            ..Default::default()
        };
        let effective = resolve_effective_tools(&policy, &all_tools());
        assert!(!effective.contains(&"bash".to_string()));
        assert!(!effective.contains(&"web_search".to_string()));
        assert!(!effective.contains(&"web_fetch".to_string()));
        assert!(effective.contains(&"read".to_string()));
    }

    #[test]
    fn test_also_allow_adds_to_profile() {
        let policy = ToolPolicy {
            profile: ToolProfile::Minimal,
            also_allow: vec!["bash".into()],
            ..Default::default()
        };
        let effective = resolve_effective_tools(&policy, &all_tools());
        assert!(effective.contains(&"session_status".to_string()));
        assert!(effective.contains(&"bash".to_string()));
        assert_eq!(effective.len(), 2);
    }

    #[test]
    fn test_explicit_allow_overrides_profile() {
        let policy = ToolPolicy {
            profile: ToolProfile::Coding,
            allow: vec!["read".into(), "write".into()],
            ..Default::default()
        };
        let effective = resolve_effective_tools(&policy, &all_tools());
        assert_eq!(effective, vec!["read", "write"]);
    }

    #[test]
    fn test_is_tool_allowed() {
        let policy = ToolPolicy {
            profile: ToolProfile::Minimal,
            ..Default::default()
        };
        assert!(is_tool_allowed("session_status", &policy, &all_tools()));
        assert!(!is_tool_allowed("bash", &policy, &all_tools()));
    }
}
