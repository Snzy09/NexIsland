// Skills loader and registry for Coucou AI Chat.
//
// Skills are specialized instructions and domain prompts stored as Markdown files:
//   - %APPDATA%\Coucou\skills\*.md (or ~/.config/coucou/skills/*.md on Linux)
//   - Or subfolders with SKILL.md
//
// Supports YAML frontmatter for metadata:
//   ---
//   name: Code Reviewer
//   description: Reviews code for security, correctness, and style.
//   ---
//   Instructions...

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::platform;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub instructions: String,
}

/// Directory where user skills are stored.
pub fn skills_dir() -> PathBuf {
    platform::config_dir().join("skills")
}

/// Ensures the skills directory exists and writes default starter skills if empty.
pub fn ensure_skills_dir() {
    let dir = skills_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    // Check if directory is empty
    let is_empty = fs::read_dir(&dir)
        .map(|mut it| it.next().is_none())
        .unwrap_or(true);

    if is_empty {
        let review_skill = r#"---
name: Code Reviewer
description: Analyzes code for bugs, edge cases, performance, and best practices.
---
You are an expert code reviewer.
When reviewing code:
1. Examine architecture, correctness, and edge cases.
2. Identify performance bottlenecks or security vulnerabilities.
3. Suggest clear, idiomatic improvements with concise explanations and code snippets.
4. Keep feedback constructive, clear, and actionable.
"#;
        let summarizer_skill = r#"---
name: Executive Summarizer
description: Condenses technical texts, transcripts, or notes into bulleted executive summaries.
---
You are an executive assistant and technical summarizer.
When summarizing content:
1. Extract the main purpose and core takeaway in 1-2 sentences.
2. Outline key points and decisions in concise bullet points.
3. List explicit action items and follow-ups with owners if mentioned.
4. Avoid fluff and keep the summary direct and high-signal.
"#;
        let _ = fs::write(dir.join("code-reviewer.md"), review_skill);
        let _ = fs::write(dir.join("summarizer.md"), summarizer_skill);
    }
}

/// Parses a skill from markdown text with optional YAML frontmatter.
pub fn parse_skill(id: &str, content: &str) -> Skill {
    let trimmed = content.trim();
    if trimmed.starts_with("---") {
        if let Some(end_idx) = trimmed[3..].find("---") {
            let frontmatter = &trimmed[3..3 + end_idx];
            let body = trimmed[3 + end_idx + 3..].trim();

            let mut name = id.to_string();
            let mut description = String::new();

            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name:") {
                    name = val.trim().trim_matches('"').trim_matches('\'').to_string();
                } else if let Some(val) = line.strip_prefix("description:") {
                    description = val.trim().trim_matches('"').trim_matches('\'').to_string();
                }
            }

            return Skill {
                id: id.to_string(),
                name,
                description,
                instructions: body.to_string(),
            };
        }
    }

    // Fallback if no frontmatter: use first line or filename
    let lines: Vec<&str> = trimmed.lines().collect();
    let name = lines
        .first()
        .map(|l| l.trim_start_matches('#').trim())
        .filter(|l| !l.is_empty())
        .unwrap_or(id)
        .to_string();

    let description = lines
        .iter()
        .skip(1)
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("")
        .to_string();

    Skill {
        id: id.to_string(),
        name,
        description,
        instructions: trimmed.to_string(),
    }
}

/// Lists all available skills.
pub fn list_skills() -> Vec<Skill> {
    ensure_skills_dir();
    let dir = skills_dir();
    let mut skills = Vec::new();

    let Ok(entries) = fs::read_dir(dir) else {
        return skills;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            if let Ok(content) = fs::read_to_string(&path) {
                skills.push(parse_skill(stem, &content));
            }
        } else if path.is_dir() {
            let skill_file = path.join("SKILL.md");
            if skill_file.exists() {
                let stem = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
                if let Ok(content) = fs::read_to_string(&skill_file) {
                    skills.push(parse_skill(stem, &content));
                }
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Retrieves a skill by ID or name (case-insensitive).
pub fn find_skill(query: &str) -> Option<Skill> {
    let q = query.trim().to_lowercase();
    list_skills().into_iter().find(|s| s.id.to_lowercase() == q || s.name.to_lowercase() == q)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_skill() {
        let text = r#"---
name: Security Auditor
description: Checks for OWASP vulnerabilities.
---
Check for SQL injection and XSS."#;
        let skill = parse_skill("auditor", text);
        assert_eq!(skill.name, "Security Auditor");
        assert_eq!(skill.description, "Checks for OWASP vulnerabilities.");
        assert_eq!(skill.instructions, "Check for SQL injection and XSS.");
    }
}
