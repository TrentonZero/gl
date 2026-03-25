use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone)]
pub struct StackInfo {
    pub stacks: Vec<Stack>,
    #[allow(dead_code)]
    pub standalone: Vec<String>,
    pub branch_to_parent: HashMap<String, String>,
    pub(crate) stale_branches: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct Stack {
    pub name: String,
    pub branches: Vec<String>,
}

impl StackInfo {
    #[allow(dead_code)]
    pub fn stack_for_branch(&self, branch: &str) -> Option<&Stack> {
        self.stacks
            .iter()
            .find(|s| s.branches.iter().any(|b| b == branch))
    }

    pub fn is_stale(&self, branch: &str) -> bool {
        self.stale_branches.contains(branch)
    }

    pub fn empty() -> Self {
        Self {
            stacks: vec![],
            standalone: vec![],
            branch_to_parent: HashMap::new(),
            stale_branches: HashSet::new(),
        }
    }
}

pub fn detect_stacks(root: &Path) -> StackInfo {
    let output = match gt_log_short(root) {
        Some(output) => output,
        None => return StackInfo::empty(),
    };

    let branches = parse_branch_names(&output);
    if branches.is_empty() {
        return StackInfo::empty();
    }

    let branch_to_parent = load_branch_parents(root.to_path_buf(), branches.clone());
    if branch_to_parent.is_empty() {
        return build_stacks_from_order(&branches);
    }

    build_stacks_from_parents(&branches, &branch_to_parent)
}

pub fn enrich_stacks(root: &Path, stack_info: &StackInfo) -> StackInfo {
    if stack_info.stacks.is_empty() {
        return stack_info.clone();
    }

    let branches: Vec<String> = stack_info
        .stacks
        .iter()
        .flat_map(|stack| stack.branches.iter().cloned())
        .collect();
    let branch_to_parent = load_branch_parents(root.to_path_buf(), branches);

    let mut enriched = stack_info.clone();
    enriched.branch_to_parent = branch_to_parent.clone();
    enriched.stale_branches = compute_stale_branches(root.to_path_buf(), branch_to_parent);
    enriched
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

fn load_branch_parents(root: PathBuf, branches: Vec<String>) -> HashMap<String, String> {
    let mut branch_to_parent = HashMap::new();
    for (branch, parent) in run_in_worker_pool(branches, move |branch| {
        gt_branch_parent(&root, &branch).map(|parent| (branch, parent))
    }) {
        branch_to_parent.insert(branch, parent);
    }

    branch_to_parent
}

/// Extract branch names from `gt log short` output, stripping ANSI codes and decorative glyphs.
pub(crate) fn parse_branch_names(output: &str) -> Vec<String> {
    let stripped = strip_ansi(output);
    let mut branches = Vec::new();

    for line in stripped.lines() {
        let Some(name) = line.split_whitespace().last() else {
            continue;
        };

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
#[allow(dead_code)]
pub(crate) fn build_stacks_from_parents(
    branches: &[String],
    branch_to_parent: &HashMap<String, String>,
) -> StackInfo {
    let trunk = detect_trunk(branches);
    let mut visited: HashMap<String, bool> = HashMap::new();
    let mut stacks: Vec<Stack> = Vec::new();
    let mut standalone: Vec<String> = Vec::new();

    for branch in branches {
        if Some(branch.as_str()) == trunk.as_deref() {
            standalone.push(branch.clone());
            continue;
        }
        if visited.contains_key(branch) {
            continue;
        }

        // Walk up the parent chain to find the full stack
        let mut chain = vec![branch.clone()];
        let mut current = branch.clone();
        while let Some(parent) = branch_to_parent.get(&current) {
            if Some(parent.as_str()) == trunk.as_deref() {
                break;
            }
            if visited.contains_key(parent) {
                break;
            }
            chain.push(parent.clone());
            current = parent.clone();
        }

        // Also walk down to find children
        let mut children_of: HashMap<String, Vec<String>> = HashMap::new();
        for (child, parent) in branch_to_parent {
            children_of
                .entry(parent.clone())
                .or_default()
                .push(child.clone());
        }

        // Rebuild the full stack from root
        let stack_root = chain.last().unwrap().clone();
        let mut ordered_stack = Vec::new();
        let mut queue = vec![stack_root];
        while let Some(node) = queue.pop() {
            if visited.contains_key(&node) {
                continue;
            }
            visited.insert(node.clone(), true);
            ordered_stack.push(node.clone());
            if let Some(children) = children_of.get(&node) {
                for child in children {
                    queue.push(child.clone());
                }
            }
        }

        if ordered_stack.len() > 1 {
            let name = format!("{} stack", ordered_stack.first().unwrap());
            stacks.push(Stack {
                name,
                branches: ordered_stack,
            });
        } else if ordered_stack.len() == 1 {
            standalone.push(ordered_stack.into_iter().next().unwrap());
        }
    }

    StackInfo {
        stacks,
        standalone,
        branch_to_parent: branch_to_parent.clone(),
        stale_branches: HashSet::new(),
    }
}

pub(crate) fn build_stacks_from_order(branches: &[String]) -> StackInfo {
    let trunk = detect_trunk(branches);
    let mut stacks = Vec::new();
    let mut standalone = Vec::new();
    let mut current_run = Vec::new();

    let flush_run =
        |run: &mut Vec<String>, stacks: &mut Vec<Stack>, standalone: &mut Vec<String>| {
            if run.is_empty() {
                return;
            }

            if run.len() > 1 {
                let ordered: Vec<String> = run.iter().rev().cloned().collect();
                let name = format!("{} stack", ordered.first().unwrap());
                stacks.push(Stack {
                    name,
                    branches: ordered,
                });
            } else {
                standalone.push(run[0].clone());
            }
            run.clear();
        };

    for branch in branches {
        if Some(branch.as_str()) == trunk.as_deref() {
            flush_run(&mut current_run, &mut stacks, &mut standalone);
            standalone.push(branch.clone());
        } else {
            current_run.push(branch.clone());
        }
    }

    flush_run(&mut current_run, &mut stacks, &mut standalone);

    StackInfo {
        stacks,
        standalone,
        branch_to_parent: HashMap::new(),
        stale_branches: HashSet::new(),
    }
}

fn compute_stale_branches(
    root: PathBuf,
    branch_to_parent: HashMap<String, String>,
) -> HashSet<String> {
    let mut stale_branches = HashSet::new();
    for branch in run_in_worker_pool(
        branch_to_parent.into_iter().collect(),
        move |(branch, parent)| {
            let Ok(parent_tip) = git_output(&root, &["rev-parse", &parent]) else {
                return None;
            };
            let Ok(merge_base) = git_output(&root, &["merge-base", &branch, &parent]) else {
                return None;
            };
            (merge_base != parent_tip).then_some(branch)
        },
    ) {
        stale_branches.insert(branch);
    }

    stale_branches
}

fn run_in_worker_pool<I, O, F>(items: Vec<I>, worker: F) -> Vec<O>
where
    I: Send + 'static,
    O: Send + 'static,
    F: Fn(I) -> Option<O> + Send + Sync + 'static,
{
    if items.is_empty() {
        return Vec::new();
    }

    let worker_count = worker_pool_size(items.len());
    let queue = Arc::new(Mutex::new(items));
    let worker = Arc::new(worker);
    let mut handles = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let worker = Arc::clone(&worker);
        handles.push(thread::spawn(move || {
            let mut results = Vec::new();
            loop {
                let next_item = {
                    let mut queue = queue.lock().ok()?;
                    queue.pop()
                };

                let Some(item) = next_item else {
                    break;
                };

                if let Some(output) = worker(item) {
                    results.push(output);
                }
            }
            Some(results)
        }));
    }

    let mut outputs = Vec::new();
    for handle in handles {
        if let Ok(Some(mut results)) = handle.join() {
            outputs.append(&mut results);
        }
    }

    outputs
}

fn worker_pool_size(job_count: usize) -> usize {
    let available = thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(4);
    available.clamp(2, 8).min(job_count)
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

    #[test]
    fn parse_branch_names_with_graph_prefixes() {
        let output = "\
◉    fix-stacked-branch-name-truncation
◯    perf-gt-tuning
◯    perf-borrow-diff-lines
◯    perf-event-driven-redraw
◯    perf-stale-metadata
◯    perf-batched-refresh
│ ◯  plan-docs-update
│ ◯  stack-view
│ ◯  stack-model-cleanup
◯─┘  main
";
        let names = parse_branch_names(output);
        assert_eq!(
            names,
            vec![
                "fix-stacked-branch-name-truncation",
                "perf-gt-tuning",
                "perf-borrow-diff-lines",
                "perf-event-driven-redraw",
                "perf-stale-metadata",
                "perf-batched-refresh",
                "plan-docs-update",
                "stack-view",
                "stack-model-cleanup",
                "main",
            ]
        );
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

    #[test]
    fn build_stacks_from_order_groups_contiguous_non_trunk_runs() {
        let branches = vec![
            "feat/top".into(),
            "feat/base".into(),
            "main".into(),
            "fix".into(),
        ];

        let info = build_stacks_from_order(&branches);
        assert_eq!(info.stacks.len(), 1);
        assert_eq!(info.stacks[0].branches, vec!["feat/base", "feat/top"]);
        assert_eq!(info.standalone, vec!["main", "fix"]);
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
            stale_branches: HashSet::new(),
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
            stale_branches: HashSet::new(),
        };
        assert!(info.stack_for_branch("main").is_none());
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
