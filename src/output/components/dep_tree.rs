use crate::format::{format_status_icon, sanitize_terminal_inline};
use crate::model::Issue;
use crate::output::Theme;
use rich_rust::prelude::*;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

/// Renders a dependency tree for an issue.
pub struct DependencyTree<'a> {
    root_issue: &'a Issue,
    all_issues: &'a [Issue],
    theme: &'a Theme,
    max_depth: usize,
}

impl<'a> DependencyTree<'a> {
    #[must_use]
    pub fn new(root: &'a Issue, all: &'a [Issue], theme: &'a Theme) -> Self {
        Self {
            root_issue: root,
            all_issues: all,
            theme,
            max_depth: 10,
        }
    }

    #[must_use]
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    #[must_use]
    pub fn build(&self) -> Tree {
        let root_node = self.build_node(self.root_issue, 0);

        Tree::new(root_node)
            .guides(TreeGuides::Rounded)
            .guide_style(self.theme.dimmed.clone())
    }

    fn build_node(&self, issue: &Issue, depth: usize) -> TreeNode {
        // Create label with ID, status, and title
        let label = format!(
            "{} [{}] {}",
            sanitize_terminal_inline(&issue.id),
            format_status_icon(&issue.status),
            truncate(&issue.title, 40)
        );

        let mut node = TreeNode::new(Text::new(label));

        // Recursively add dependencies (if not too deep)
        if depth < self.max_depth {
            for dep in &issue.dependencies {
                if let Some(dep_issue) = self.find_issue(&dep.depends_on_id) {
                    let child = self.build_node(dep_issue, depth + 1);
                    node = node.child(child);
                } else {
                    // Dependency not found (external or deleted)
                    let missing = TreeNode::new(Text::new(format!(
                        "{} [?] (not found)",
                        sanitize_terminal_inline(&dep.depends_on_id)
                    )));
                    node = node.child(missing);
                }
            }
        }

        node
    }

    fn find_issue(&self, id: &str) -> Option<&Issue> {
        self.all_issues.iter().find(|i| i.id == id)
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = sanitize_terminal_inline(s);
    let s = s.as_ref();

    let width = UnicodeWidthStr::width(s);
    if width <= max {
        s.to_string()
    } else if max <= 3 {
        let mut w = 0;
        let mut out = String::new();
        for c in s.chars() {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if w + cw > max {
                break;
            }
            w += cw;
            out.push(c);
        }
        out
    } else {
        let target = max - 3;
        let mut w = 0;
        let mut out = String::new();
        for c in s.chars() {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if w + cw > target {
                break;
            }
            w += cw;
            out.push(c);
        }
        out.push_str("...");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Dependency, DependencyType, Status};
    use crate::output::Theme;
    use chrono::Utc;

    #[test]
    fn test_dep_tree_truncation_safe() {
        // Emojis are 2 visual columns each
        let title = "😊".repeat(20); // 20 chars, 40 visual columns
        // Max 10 visual columns → 3 emojis (6 cols) + "..." (3 cols) = 9 cols
        let truncated = truncate(&title, 10);

        assert!(UnicodeWidthStr::width(truncated.as_str()) <= 10);
        assert!(truncated.starts_with("😊"));
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_dep_tree_truncation_ascii() {
        let title = "Hello, World! This is a long title";
        let truncated = truncate(title, 15);
        assert!(UnicodeWidthStr::width(truncated.as_str()) <= 15);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_dep_tree_truncation_short() {
        let title = "Short";
        let truncated = truncate(title, 40);
        assert_eq!(truncated, "Short");
    }

    #[test]
    fn dependency_tree_sanitizes_labels_and_uses_safe_status_icon() {
        let mut issue = Issue {
            id: "bd-root".to_string(),
            title: "Root\x1b[2J".to_string(),
            status: Status::Custom("\x1b[31mhidden".to_string()),
            ..Issue::default()
        };
        issue.dependencies.push(Dependency {
            issue_id: issue.id.clone(),
            depends_on_id: "external:proj:\x07capability".to_string(),
            dep_type: DependencyType::Blocks,
            created_at: Utc::now(),
            created_by: None,
            metadata: None,
            thread_id: None,
        });

        let theme = Theme::default();
        let rendered = DependencyTree::new(&issue, &[], &theme)
            .build()
            .render_plain();

        assert!(!rendered.contains('\x1b'));
        assert!(!rendered.contains('\x07'));
        assert!(rendered.contains("bd-root [?] Root\\u{1b}[2J"));
        assert!(rendered.contains("external:proj:\\u{7}capability [?] (not found)"));
    }
}
