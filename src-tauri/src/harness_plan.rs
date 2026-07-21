//! Parser for the harness plan file (`.gospel/PLAN.md`).
//!
//! The plan file is a plain UTF-8 markdown document whose structure is
//! documented in `workspace_tools.rs` (Harness Control Area) and `CONTEXT.md`
//! ("Explicit Planning Mechanism" / "PEV Loop"). The canonical section
//! headings used by this parser are:
//!
//! - `## Goal` — a single paragraph (the first paragraph after the heading).
//! - `## Steps` — a markdown checklist (`- [ ]` / `- [x]`).
//! - `## Evidence / Verification` — paragraphs (one entry per non-empty line
//!   or block; collected as a list of strings).
//! - `## Open Questions / Risks` — paragraphs collected as a list of strings.
//! - `## Next Action` — a single paragraph (the first paragraph after the
//!   heading; may contain a one-item checklist).
//!
//! Heading style is canonicalised to `## <NAME>` (two leading `#`). A `# Goal`
//! (single-hash) form is also tolerated for the Goal section only, because
//! early plan examples used it; the parser records which form was seen in
//! `has_plan_file` semantics only — both parse to the same `PlanFile` shape.
//!
//! The parser is intentionally line-based: the file contract is stable section
//! headings only, so no full markdown AST is required.

use serde::{Deserialize, Serialize};

/// One checklist step under `## Steps`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub text: String,
    pub done: bool,
}

/// Parsed contents of `.gospel/PLAN.md`.
///
/// When the plan file does not exist on disk, the Tauri command returns a
/// `PlanFile` with `has_plan_file == false` and all other fields empty/None.
/// Callers should treat that as the "no plan yet" sentinel.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PlanFile {
    pub goal: Option<String>,
    pub steps: Vec<PlanStep>,
    pub evidence: Vec<String>,
    pub open_questions: Vec<String>,
    pub next_action: Option<String>,
    pub has_plan_file: bool,
}

/// Section kinds the parser knows how to extract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Goal,
    Steps,
    Evidence,
    OpenQuestions,
    NextAction,
}

/// Parse a markdown plan document into a [`PlanFile`].
///
/// `has_plan_file` is set to `true` by the caller (the Tauri command) once it
/// has confirmed the file exists on disk; this pure function does not touch
/// the filesystem and leaves `has_plan_file` at its default `false`.
pub fn parse_plan_markdown(content: &str) -> PlanFile {
    let mut plan = PlanFile::default();
    let mut section = Section::None;
    let mut paragraph: Vec<String> = Vec::new();

    let mut flush_paragraph = |paragraph: &mut Vec<String>, out: &mut String| {
        let joined = paragraph
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        paragraph.clear();
        if !joined.is_empty() {
            *out = joined;
        }
    };

    let mut flush_into_list = |paragraph: &mut Vec<String>, out: &mut Vec<String>| {
        let joined = paragraph
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        paragraph.clear();
        if !joined.is_empty() {
            out.push(joined);
        }
    };

    for raw in content.lines() {
        let line = raw.trim_end();

        if let Some(heading) = parse_heading(line) {
            // Flush whatever we were collecting into the right slot.
            close_section(
                &mut plan,
                section,
                &mut paragraph,
                &mut flush_paragraph,
                &mut flush_into_list,
            );
            section = heading;
            continue;
        }

        match section {
            Section::None => {
                // Preamble before any heading — ignored.
            }
            Section::Goal => {
                if line.trim().is_empty() {
                    let mut g = String::new();
                    flush_paragraph(&mut paragraph, &mut g);
                    if !g.is_empty() && plan.goal.is_none() {
                        plan.goal = Some(g);
                    }
                } else {
                    paragraph.push(line.to_string());
                }
            }
            Section::Steps => {
                if let Some((done, text)) = parse_checklist_item(line) {
                    plan.steps.push(PlanStep {
                        text: text.to_string(),
                        done,
                    });
                }
            }
            Section::Evidence => {
                if line.trim().is_empty() {
                    flush_into_list(&mut paragraph, &mut plan.evidence);
                } else if line.trim().starts_with('-') && !line.trim().starts_with("- [") {
                    // Bullet list — push each as its own evidence entry.
                    let text = line.trim().trim_start_matches('-').trim().to_string();
                    if !text.is_empty() {
                        plan.evidence.push(text);
                    }
                } else {
                    paragraph.push(line.to_string());
                }
            }
            Section::OpenQuestions => {
                if line.trim().is_empty() {
                    flush_into_list(&mut paragraph, &mut plan.open_questions);
                } else if line.trim().starts_with('-') && !line.trim().starts_with("- [") {
                    let text = line.trim().trim_start_matches('-').trim().to_string();
                    if !text.is_empty() {
                        plan.open_questions.push(text);
                    }
                } else {
                    paragraph.push(line.to_string());
                }
            }
            Section::NextAction => {
                if line.trim().is_empty() {
                    let mut g = String::new();
                    flush_paragraph(&mut paragraph, &mut g);
                    if !g.is_empty() && plan.next_action.is_none() {
                        plan.next_action = Some(g);
                    }
                } else if let Some((done, text)) = parse_checklist_item(line) {
                    if plan.next_action.is_none() {
                        plan.next_action = Some(format!(
                            "{}{}",
                            if done { "[x] " } else { "[ ] " },
                            text
                        ));
                    }
                } else {
                    paragraph.push(line.to_string());
                }
            }
        }
    }

    // Flush trailing section.
    close_section(
        &mut plan,
        section,
        &mut paragraph,
        &mut flush_paragraph,
        &mut flush_into_list,
    );

    plan
}

fn close_section(
    plan: &mut PlanFile,
    section: Section,
    paragraph: &mut Vec<String>,
    flush_paragraph: &mut impl FnMut(&mut Vec<String>, &mut String),
    flush_into_list: &mut impl FnMut(&mut Vec<String>, &mut Vec<String>),
) {
    match section {
        Section::None => {
            paragraph.clear();
        }
        Section::Goal => {
            let mut g = String::new();
            flush_paragraph(paragraph, &mut g);
            if !g.is_empty() && plan.goal.is_none() {
                plan.goal = Some(g);
            }
        }
        Section::Steps => {
            paragraph.clear();
        }
        Section::Evidence => {
            flush_into_list(paragraph, &mut plan.evidence);
        }
        Section::OpenQuestions => {
            flush_into_list(paragraph, &mut plan.open_questions);
        }
        Section::NextAction => {
            let mut g = String::new();
            flush_paragraph(paragraph, &mut g);
            if !g.is_empty() && plan.next_action.is_none() {
                plan.next_action = Some(g);
            }
        }
    }
}

/// Returns `Some((Section, heading_level))` if the line is a recognised
/// section heading, otherwise `None`. Recognised headings:
/// - `# Goal` / `## Goal`
/// - `## Steps`
/// - `## Evidence / Verification` (the slash form is the documented one; we
///   also accept `## Evidence` and `## Verification` as aliases)
/// - `## Open Questions / Risks` (and aliases `## Open Questions`,
///   `## Risks`)
/// - `## Next Action`
fn parse_heading(line: &str) -> Option<Section> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes == 0 {
        return None;
    }
    if hashes > 3 {
        return None;
    }
    let name = trimmed[hashes..].trim().to_lowercase();
    if name.is_empty() {
        return None;
    }
    match name.as_str() {
        "goal" => Some(Section::Goal),
        "steps" => Some(Section::Steps),
        "evidence / verification" | "evidence / verifications" | "evidence"
        | "verification" => Some(Section::Evidence),
        "open questions / risks" | "open questions" | "risks" => Some(Section::OpenQuestions),
        "next action" => Some(Section::NextAction),
        _ => None,
    }
}

/// Parse a markdown checklist line (`- [ ]` / `- [x]` / `* [ ]`). Returns
/// `Some((done, text))` if the line is a checklist item, else `None`.
fn parse_checklist_item(line: &str) -> Option<(bool, &str)> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;
    let rest = rest.trim_start();
    if let Some(after) = rest.strip_prefix("[x]") {
        return Some((true, after.trim()));
    }
    if let Some(after) = rest.strip_prefix("[X]") {
        return Some((true, after.trim()));
    }
    if let Some(after) = rest.strip_prefix("[ ]") {
        return Some((false, after.trim()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_plan() -> &'static str {
        "# Plan

## Goal

Implement Phase 1 of the shell tools.

## Steps

- [x] Read the handoff and explore the codebase.
- [x] Implement shell_tools.rs.
- [ ] Wire the tools into llm.rs.
- [ ] Verify with cargo test.

## Evidence / Verification

- `cargo test shell_tools` — 21 tests passed.
- `bun run build` — Vite frontend build succeeded.

## Open Questions / Risks

- Approval timeout set to 60 seconds.
- Classification engine is conservative.

## Next Action

Ready for review. Future phases can add overrides.
"
    }

    #[test]
    fn happy_path_all_sections_present() {
        let plan = parse_plan_markdown(full_plan());
        assert_eq!(
            plan.goal.as_deref(),
            Some("Implement Phase 1 of the shell tools.")
        );
        assert_eq!(plan.steps.len(), 4);
        assert!(plan.steps[0].done);
        assert!(plan.steps[1].done);
        assert!(!plan.steps[2].done);
        assert!(!plan.steps[3].done);
        assert_eq!(
            plan.steps[2].text,
            "Wire the tools into llm.rs."
        );
        assert_eq!(plan.evidence.len(), 2);
        assert!(plan.evidence[0].contains("cargo test shell_tools"));
        assert_eq!(plan.open_questions.len(), 2);
        assert!(plan.open_questions[0].contains("Approval timeout"));
        assert_eq!(
            plan.next_action.as_deref(),
            Some("Ready for review. Future phases can add overrides.")
        );
        assert!(!plan.has_plan_file);
    }

    #[test]
    fn missing_file_sentinel_is_default() {
        let plan = parse_plan_markdown("");
        assert!(!plan.has_plan_file);
        assert!(plan.goal.is_none());
        assert!(plan.steps.is_empty());
        assert!(plan.evidence.is_empty());
        assert!(plan.open_questions.is_empty());
        assert!(plan.next_action.is_none());
    }

    #[test]
    fn partial_plan_goal_and_steps_only() {
        let content = "## Goal\n\nShip the spike.\n\n## Steps\n\n- [ ] Investigate.\n- [x] Write parser.\n";
        let plan = parse_plan_markdown(content);
        assert_eq!(plan.goal.as_deref(), Some("Ship the spike."));
        assert_eq!(plan.steps.len(), 2);
        assert!(!plan.steps[0].done);
        assert!(plan.steps[1].done);
        assert!(plan.evidence.is_empty());
        assert!(plan.open_questions.is_empty());
        assert!(plan.next_action.is_none());
    }

    #[test]
    fn mixed_heading_styles_accepted() {
        // Goal under single-hash `# Goal`; other sections under `## ...`.
        // The parser canonicalises both to the same PlanFile shape.
        let content = "# Goal\n\nMixed heading plan.\n\n## Steps\n\n- [x] Done.\n\n## Next Action\n\nShip it.\n";
        let plan = parse_plan_markdown(content);
        assert_eq!(plan.goal.as_deref(), Some("Mixed heading plan."));
        assert_eq!(plan.steps.len(), 1);
        assert!(plan.steps[0].done);
        assert_eq!(plan.next_action.as_deref(), Some("Ship it."));
    }

    #[test]
    fn next_action_checklist_form_captured() {
        let content = "## Next Action\n\n- [ ] Open PR for the parser.\n";
        let plan = parse_plan_markdown(content);
        assert_eq!(
            plan.next_action.as_deref(),
            Some("[ ] Open PR for the parser.")
        );
    }

    #[test]
    fn unknown_headings_do_not_reset_paragraph_into_wrong_slot() {
        let content = "## Goal\n\nThe goal.\n\n## Notes\n\nSome notes.\n\n## Steps\n\n- [ ] One.\n";
        let plan = parse_plan_markdown(content);
        assert_eq!(plan.goal.as_deref(), Some("The goal."));
        // Unknown headings just leave the section as Goal until a known one
        // appears; the paragraph under "## Notes" is flushed into Goal only
        // if Goal is still empty — it is not, so Notes is dropped, matching
        // the documented "first paragraph under ## Goal" rule.
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].text, "One.");
    }
}