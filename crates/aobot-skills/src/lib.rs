//! aobot-skills: Skill loading, frontmatter parsing, and slash commands.
//!
//! Skills are Markdown files with YAML frontmatter that define reusable
//! agent behaviors. They can be invoked as slash commands (e.g. `/review-pr`).
//!
//! # Skill file format
//!
//! ```markdown
//! ---
//! name: review-pr
//! description: Review a GitHub pull request
//! allowed_tools: [bash, read, grep, find]
//! user_invocable: true
//! ---
//!
//! # Review PR
//!
//! [Markdown instructions injected as system prompt]
//! ```

pub mod commands;
pub mod frontmatter;
pub mod loader;

pub use commands::SkillCommand;
pub use loader::{SkillEntry, SkillSource};
