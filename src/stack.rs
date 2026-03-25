use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct StackInfo {
    pub stacks: Vec<Stack>,
    pub standalone: Vec<String>,
    pub branch_to_parent: HashMap<String, String>,
    pub status: StackStatus,
}

#[derive(Debug, Clone)]
pub struct Stack {
    pub name: String,
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackStatus {
    Available,
    Degraded { message: String },
}

impl StackInfo {
    pub fn stack_for_branch(&self, branch: &str) -> Option<&Stack> {
        self.stacks
            .iter()
            .find(|s| s.branches.iter().any(|b| b == branch))
    }

    pub fn notice(&self) -> Option<&str> {
        match &self.status {
            StackStatus::Available => None,
            StackStatus::Degraded { message } => Some(message.as_str()),
        }
    }

    pub fn is_stale(&self, root: &Path, branch: &str) -> bool {
        let Some(parent) = self.branch_to_parent.get(branch) else {
            return false;
        };
        let Ok(merge_base) = git_output(root, &["merge-base", branch, parent]) else {
            return false;
        };
        let Ok(parent_tip) = git_output(root, &["rev-parse", parent]) else {
            return false;
        };
        merge_base != parent_tip
    }
}

pub fn detect_stacks(root: &Path) -> StackInfo {
    if !gt_available() {
        return StackInfo {
            stacks: vec![],
            standalone: vec![],
            branch_to_parent: HashMap::new(),
            status: StackStatus::Degraded {
                message: "Graphite CLI not found; showing a flat branch list.".to_string(),
            },
        };
    }

    let output = match gt_log_short(root) {
        Some(output) => output,
        None => {
            return StackInfo {
                stacks: vec![],
                standalone: vec![],
                branch_to_parent: HashMap::new(),
                status: StackStatus::Degraded {
                    message: "Graphite stack metadata unavailable; showing a flat branch list."
                        .to_string(),
                },
            }
        }
    };

    parse_gt_log(&output, root)
}

fn gt_available() -> bool {
    Command::new("gt")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn gt_log_short(root: &Path) -> Option<String> {
    let output = Command::new("gt")
        .args(["log", "short", "--no-interactive"])
        .current_dir(root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn gt_branch_parent(root: &Path, branch: &str) -> Option<String> {
    let output = Command::new("gt")
        .args(["branch", "info", branch, "--no-interactive"])
        .current_dir(root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    // gt branch info shows parent in the output, look for "Parent" line
    // or we can infer from gt log structure
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Parent:") {
            return Some(rest.trim().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("parent:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Parse `gt log short` output to extract stack structure.
///
/// The output format looks like:
/// ```
/// ◉  feature/auth-ui
/// ◉  feature/auth-middleware
/// ◉  feature/auth-base
/// ◉  main
/// ```
///
/// Branches above trunk (main/master) that appear in a contiguous sequence
/// form a stack. The trunk branch itself is standalone.
fn parse_gt_log(output: &str, root: &Path) -> StackInfo {
    let branches = parse_branch_names(output);

    // Build parent relationships using gt branch info for each non-trunk branch
    let trunk = detect_trunk(&branches);
    let mut branch_to_parent: HashMap<String, String> = HashMap::new();
    for branch in &branches {
        if Some(branch.as_str()) == trunk.as_deref() {
            continue;
        }
        if let Some(parent) = gt_branch_parent(root, branch) {
            branch_to_parent.insert(branch.clone(), parent);
        }
    }

    build_stacks_from_parents(&branches, &branch_to_parent)
}

/// Extract branch names from `gt log short` output, stripping ANSI codes and decorative glyphs.
pub(crate) fn parse_branch_names(output: &str) -> Vec<String> {
    let stripped = strip_ansi(output);
    let mut branches = Vec::new();

    for line in stripped.lines() {
        let trimmed = line.trim();
        let name = trimmed
            .trim_start_matches('◉')
            .trim_start_matches('●')
            .trim_start_matches('◯')
            .trim_start_matches('○')
            .trim_start_matches('│')
            .trim_start_matches('├')
            .trim_start_matches('└')
            .trim_start_matches('─')
            .trim_start_matches('|')
            .trim_start_matches('-')
            .trim();

        if name.is_empty() {
            continue;
        }

        if name
            .chars()
            .all(|c| !c.is_alphanumeric() && c != '/' && c != '-' && c != '_')
        {
            continue;
        }

        branches.push(name.to_string());
    }

    branches
}

/// Detect trunk branch from a list of branch names.
pub(crate) fn detect_trunk(branches: &[String]) -> Option<String> {
    for candidate in &["main", "master", "trunk", "develop"] {
        if branches.iter().any(|b| b == *candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Build stack groupings from a list of branches and their parent relationships.
pub(crate) fn build_stacks_from_parents(
    branches: &[String],
    branch_to_parent: &HashMap<String, String>,
) -> StackInfo {
    let trunk = detect_trunk(branches);
    let branch_order: HashMap<&str, usize> = branches
        .iter()
        .enumerate()
        .map(|(idx, branch)| (branch.as_str(), idx))
        .collect();
    let mut children_of: HashMap<String, Vec<String>> = HashMap::new();
    for (child, parent) in branch_to_parent {
        children_of
            .entry(parent.clone())
            .or_default()
            .push(child.clone());
    }
    for children in children_of.values_mut() {
        children.sort_by_key(|child| {
            branch_order
                .get(child.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
    }

    let mut stacks = Vec::new();
    let mut standalone = Vec::new();
    let mut assigned = std::collections::HashSet::new();

    for branch in branches {
        if Some(branch.as_str()) == trunk.as_deref() {
            standalone.push(branch.clone());
            assigned.insert(branch.clone());
            continue;
        }

        let Some(parent) = branch_to_parent.get(branch) else {
            standalone.push(branch.clone());
            assigned.insert(branch.clone());
            continue;
        };

        let is_stack_root =
            Some(parent.as_str()) == trunk.as_deref() || !branch_to_parent.contains_key(parent);
        if !is_stack_root || assigned.contains(branch) {
            continue;
        }

        let mut chain = vec![branch.clone()];
        assigned.insert(branch.clone());
        let mut cursor = branch.clone();

        loop {
            let Some(children) = children_of.get(&cursor) else {
                break;
            };
            if children.len() != 1 {
                break;
            }

            let child = &children[0];
            if assigned.contains(child) {
                break;
            }

            chain.push(child.clone());
            assigned.insert(child.clone());
            cursor = child.clone();
        }

        if chain.len() > 1 {
            stacks.push(Stack {
                name: format!("{} stack", chain[0]),
                branches: chain,
            });
        } else {
            standalone.push(branch.clone());
        }
    }

    for branch in branches {
        if assigned.insert(branch.clone()) {
            standalone.push(branch.clone());
        }
    }

    StackInfo {
        stacks,
        standalone,
        branch_to_parent: branch_to_parent.clone(),
        status: StackStatus::Available,
    }
}

pub(crate) fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip until we hit a letter (end of ANSI sequence)
            while let Some(&next) = chars.peek() {
                chars.next();
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- strip_ansi ---

    #[test]
    fn strip_ansi_plain_text() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn strip_ansi_removes_color_codes() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
    }

    #[test]
    fn strip_ansi_complex_sequence() {
        assert_eq!(
            strip_ansi("\x1b[1;34m◉\x1b[0m  \x1b[37mmain\x1b[0m"),
            "◉  main"
        );
    }

    #[test]
    fn strip_ansi_empty() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn strip_ansi_only_escape() {
        assert_eq!(strip_ansi("\x1b[0m"), "");
    }

    // --- parse_branch_names ---

    #[test]
    fn parse_branch_names_simple() {
        let output = "◉  feature/auth-ui\n◉  feature/auth-base\n◉  main\n";
        let names = parse_branch_names(output);
        assert_eq!(names, vec!["feature/auth-ui", "feature/auth-base", "main"]);
    }

    #[test]
    fn parse_branch_names_with_ansi() {
        let output = "\x1b[32m◉\x1b[0m  \x1b[37mmain\x1b[0m\n";
        let names = parse_branch_names(output);
        assert_eq!(names, vec!["main"]);
    }

    #[test]
    fn parse_branch_names_empty() {
        assert!(parse_branch_names("").is_empty());
    }

    #[test]
    fn parse_branch_names_decorative_only() {
        let output = "│\n├──\n";
        assert!(parse_branch_names(output).is_empty());
    }

    #[test]
    fn parse_branch_names_various_markers() {
        let output = "●  checked-out\n◯  other-branch\n";
        let names = parse_branch_names(output);
        assert_eq!(names, vec!["checked-out", "other-branch"]);
    }

    #[test]
    fn parse_branch_names_with_underscores() {
        let output = "◉  my_feature_branch\n";
        let names = parse_branch_names(output);
        assert_eq!(names, vec!["my_feature_branch"]);
    }

    // --- detect_trunk ---

    #[test]
    fn detect_trunk_main() {
        let branches = vec!["feature".into(), "main".into()];
        assert_eq!(detect_trunk(&branches), Some("main".to_string()));
    }

    #[test]
    fn detect_trunk_master() {
        let branches = vec!["feature".into(), "master".into()];
        assert_eq!(detect_trunk(&branches), Some("master".to_string()));
    }

    #[test]
    fn detect_trunk_prefers_main_over_master() {
        let branches = vec!["main".into(), "master".into()];
        assert_eq!(detect_trunk(&branches), Some("main".to_string()));
    }

    #[test]
    fn detect_trunk_none() {
        let branches = vec!["feature".into(), "bugfix".into()];
        assert_eq!(detect_trunk(&branches), None);
    }

    #[test]
    fn detect_trunk_develop() {
        let branches = vec!["feature".into(), "develop".into()];
        assert_eq!(detect_trunk(&branches), Some("develop".to_string()));
    }

    // --- build_stacks_from_parents ---

    #[test]
    fn build_stacks_single_stack() {
        let branches = vec![
            "feat/auth-ui".into(),
            "feat/auth-mid".into(),
            "feat/auth-base".into(),
            "main".into(),
        ];
        let mut parents = HashMap::new();
        parents.insert("feat/auth-ui".into(), "feat/auth-mid".into());
        parents.insert("feat/auth-mid".into(), "feat/auth-base".into());
        parents.insert("feat/auth-base".into(), "main".into());

        let info = build_stacks_from_parents(&branches, &parents);
        assert_eq!(info.stacks.len(), 1);
        assert_eq!(info.stacks[0].branches.len(), 3);
        // Base should come first
        assert_eq!(info.stacks[0].branches[0], "feat/auth-base");
        assert!(info.standalone.contains(&"main".to_string()));
        assert_eq!(info.status, StackStatus::Available);
    }

    #[test]
    fn build_stacks_two_stacks() {
        let branches = vec![
            "feat/a2".into(),
            "feat/a1".into(),
            "feat/b2".into(),
            "feat/b1".into(),
            "main".into(),
        ];
        let mut parents = HashMap::new();
        parents.insert("feat/a2".into(), "feat/a1".into());
        parents.insert("feat/a1".into(), "main".into());
        parents.insert("feat/b2".into(), "feat/b1".into());
        parents.insert("feat/b1".into(), "main".into());

        let info = build_stacks_from_parents(&branches, &parents);
        assert_eq!(info.stacks.len(), 2);
        for stack in &info.stacks {
            assert_eq!(stack.branches.len(), 2);
        }
    }

    #[test]
    fn build_stacks_standalone_only() {
        let branches = vec!["fix/typo".into(), "main".into()];
        let mut parents = HashMap::new();
        parents.insert("fix/typo".into(), "main".into());

        let info = build_stacks_from_parents(&branches, &parents);
        assert!(info.stacks.is_empty());
        assert!(info.standalone.contains(&"main".to_string()));
        assert!(info.standalone.contains(&"fix/typo".to_string()));
    }

    #[test]
    fn build_stacks_branching_children_fall_back_to_standalone() {
        let branches = vec![
            "feat/base".into(),
            "feat/one".into(),
            "feat/two".into(),
            "main".into(),
        ];
        let mut parents = HashMap::new();
        parents.insert("feat/base".into(), "main".into());
        parents.insert("feat/one".into(), "feat/base".into());
        parents.insert("feat/two".into(), "feat/base".into());

        let info = build_stacks_from_parents(&branches, &parents);
        assert!(info.stacks.is_empty());
        assert!(info.standalone.contains(&"feat/base".to_string()));
        assert!(info.standalone.contains(&"feat/one".to_string()));
        assert!(info.standalone.contains(&"feat/two".to_string()));
    }

    #[test]
    fn build_stacks_no_parents() {
        let branches = vec!["feature".into(), "main".into()];
        let parents = HashMap::new();

        let info = build_stacks_from_parents(&branches, &parents);
        assert!(info.stacks.is_empty());
        assert_eq!(info.standalone.len(), 2);
    }

    #[test]
    fn build_stacks_empty() {
        let info = build_stacks_from_parents(&[], &HashMap::new());
        assert!(info.stacks.is_empty());
        assert!(info.standalone.is_empty());
    }

    // --- StackInfo::stack_for_branch ---

    #[test]
    fn stack_for_branch_found() {
        let info = StackInfo {
            stacks: vec![Stack {
                name: "auth stack".into(),
                branches: vec!["auth-base".into(), "auth-ui".into()],
            }],
            standalone: vec![],
            branch_to_parent: HashMap::new(),
            status: StackStatus::Available,
        };
        let stack = info.stack_for_branch("auth-ui");
        assert!(stack.is_some());
        assert_eq!(stack.unwrap().name, "auth stack");
    }

    #[test]
    fn stack_for_branch_not_found() {
        let info = StackInfo {
            stacks: vec![],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            status: StackStatus::Available,
        };
        assert!(info.stack_for_branch("main").is_none());
    }

    #[test]
    fn degraded_notice_is_exposed() {
        let info = StackInfo {
            stacks: vec![],
            standalone: vec![],
            branch_to_parent: HashMap::new(),
            status: StackStatus::Degraded {
                message: "degraded".into(),
            },
        };
        assert_eq!(info.notice(), Some("degraded"));
    }
}

fn git_output(root: &Path, args: &[&str]) -> Result<String, ()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
