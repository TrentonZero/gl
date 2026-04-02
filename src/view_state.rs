use crate::{
    git::{BranchInfo, RepoState, WorktreeInfo},
    stack::{Stack, StackInfo},
    ui::{BranchEntry, StackView, StackViewBranch},
};
use std::collections::{HashMap, HashSet};

pub(crate) fn build_stack_view(
    repo: &RepoState,
    stack_info: &StackInfo,
    branch_name: &str,
) -> Option<StackView> {
    let stack = stack_info.stack_for_branch(branch_name)?;
    let branch_map: HashMap<_, _> = repo
        .branches
        .iter()
        .map(|branch| (branch.name.clone(), branch))
        .collect();
    let selected_index = stack.branches.iter().position(|name| name == branch_name)?;
    let diff_branch = branch_for_diff(repo, stack_info, branch_name)?;
    let parent_branch = if selected_index > 0 {
        Some(stack.branches[selected_index - 1].clone())
    } else {
        diff_branch.base_ref.clone()
    };
    let child_branch = stack.branches.get(selected_index + 1).cloned();

    let branches = stack
        .branches
        .iter()
        .filter_map(|name| {
            let branch = branch_map.get(name.as_str())?;
            Some(StackViewBranch {
                name: name.clone(),
                is_selected: name == branch_name,
                is_head: branch.is_head,
                commit_count: branch.commit_count,
                ahead: branch.ahead,
                behind: branch.behind,
                has_upstream: branch.upstream.is_some(),
                stale: stack_info.is_stale(name),
            })
        })
        .collect();

    Some(StackView {
        title: stack.name.clone(),
        selected_branch: branch_name.to_string(),
        parent_branch,
        child_branch,
        base_ref: diff_branch.base_ref,
        stale: stack_info.is_stale(branch_name),
        branches,
    })
}

pub(crate) fn diff_preload_targets(
    repo: &RepoState,
    stack_info: &StackInfo,
    display_entries: &[BranchEntry],
) -> Vec<BranchInfo> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();

    for branch_name in display_entries
        .iter()
        .filter(|entry| !entry.is_header())
        .map(BranchEntry::branch_name)
    {
        let Some(branch) = branch_for_diff(repo, stack_info, branch_name) else {
            continue;
        };
        if seen.insert(branch.name.clone()) {
            targets.push(branch);
        }
    }

    targets
}

pub(crate) fn build_display_entries(
    repo: &RepoState,
    stack_info: &StackInfo,
    worktrees: &[WorktreeInfo],
    expanded_stacks: &HashSet<String>,
) -> Vec<BranchEntry> {
    let mut entries = Vec::new();
    let mut used_branches = HashSet::new();
    let branch_map: HashMap<_, _> = repo
        .branches
        .iter()
        .map(|branch| (&branch.name, branch))
        .collect();
    let worktree_by_branch: HashMap<_, _> = worktrees
        .iter()
        .filter_map(|worktree| {
            let branch = worktree.branch.as_ref()?;
            let label = worktree.path.file_name()?.to_string_lossy().to_string();
            Some((branch.clone(), label))
        })
        .collect();

    for stack in &stack_info.stacks {
        used_branches.extend(stack.branches.iter().cloned());
        entries.push(BranchEntry::Header {
            label: stack.name.clone(),
            expanded: Some(expanded_stacks.contains(&stack.name)),
        });
        if !expanded_stacks.contains(&stack.name) {
            continue;
        }
        for (depth, branch_name) in stack.branches.iter().enumerate() {
            if let Some(branch) = branch_map.get(branch_name) {
                let stale = stack_info.is_stale(branch_name);
                entries.push(BranchEntry::Branch {
                    branch_name: branch_name.clone(),
                    is_head: branch.is_head,
                    commit_count: branch.commit_count,
                    ahead: branch.ahead,
                    behind: branch.behind,
                    has_upstream: branch.upstream.is_some(),
                    indent: depth + 1,
                    stale,
                    worktree_label: worktree_by_branch.get(branch_name).cloned(),
                });
            }
        }
    }

    let mut standalone_names = Vec::new();
    let mut seen_standalone = HashSet::new();
    if !stack_info.standalone.is_empty() {
        for name in &stack_info.standalone {
            if branch_map.contains_key(name) && !used_branches.contains(name) {
                standalone_names.push(name.clone());
                seen_standalone.insert(name.clone());
            }
        }
    }
    standalone_names.extend(
        repo.branches
            .iter()
            .filter(|branch| !used_branches.contains(&branch.name))
            .map(|branch| branch.name.clone())
            .filter(|name| !seen_standalone.contains(name)),
    );

    if !standalone_names.is_empty() {
        if !stack_info.stacks.is_empty() {
            entries.push(BranchEntry::Header {
                label: "standalone".to_string(),
                expanded: None,
            });
        }
        for branch_name in standalone_names {
            let Some(branch) = branch_map.get(&branch_name) else {
                continue;
            };
            entries.push(BranchEntry::Branch {
                branch_name,
                is_head: branch.is_head,
                commit_count: branch.commit_count,
                ahead: branch.ahead,
                behind: branch.behind,
                has_upstream: branch.upstream.is_some(),
                indent: 0,
                stale: false,
                worktree_label: worktree_by_branch.get(&branch.name).cloned(),
            });
        }
    }

    entries
}

pub(crate) fn initial_expanded_stacks(repo: &RepoState, stack_info: &StackInfo) -> HashSet<String> {
    stack_info
        .stacks
        .iter()
        .filter(|stack| stack_contains_head(repo, stack))
        .map(|stack| stack.name.clone())
        .collect()
}

pub(crate) fn stack_name_for_branch<'a>(
    stack_info: &'a StackInfo,
    branch_name: &str,
) -> Option<&'a str> {
    stack_info
        .stacks
        .iter()
        .find(|stack| stack.branches.iter().any(|name| name == branch_name))
        .map(|stack| stack.name.as_str())
}

pub(crate) fn ordered_branch_names(repo: &RepoState, stack_info: &StackInfo) -> Vec<String> {
    let mut branches = Vec::new();
    let mut used_branches = HashSet::new();

    for stack in &stack_info.stacks {
        for branch_name in &stack.branches {
            if repo
                .branches
                .iter()
                .any(|branch| branch.name == *branch_name)
            {
                branches.push(branch_name.clone());
                used_branches.insert(branch_name.clone());
            }
        }
    }

    for branch_name in &stack_info.standalone {
        if repo
            .branches
            .iter()
            .any(|branch| branch.name == *branch_name)
            && !used_branches.contains(branch_name)
        {
            branches.push(branch_name.clone());
            used_branches.insert(branch_name.clone());
        }
    }

    for branch in &repo.branches {
        if used_branches.insert(branch.name.clone()) {
            branches.push(branch.name.clone());
        }
    }

    branches
}

pub(crate) fn branch_for_diff(
    repo: &RepoState,
    stack_info: &StackInfo,
    branch_name: &str,
) -> Option<BranchInfo> {
    let mut branch = repo
        .branches
        .iter()
        .find(|branch| branch.name == branch_name)
        .cloned()?;
    if let Some(parent) = stack_info.branch_to_parent.get(branch_name) {
        branch.base_ref = Some(parent.clone());
    }
    Some(branch)
}

pub(crate) fn stack_contains_head(repo: &RepoState, stack: &Stack) -> bool {
    stack.branches.iter().any(|branch_name| {
        repo.branches
            .iter()
            .any(|branch| branch.name == *branch_name && branch.is_head)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::RepoState;
    use std::path::PathBuf;

    fn make_branch(name: &str) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            is_head: false,
            object_id: format!("{name}-oid"),
            upstream: None,
            ahead: 0,
            behind: 0,
            commit_count: 1,
            base_ref: Some("main".to_string()),
        }
    }

    fn make_repo(branch_names: &[&str]) -> RepoState {
        RepoState {
            root: PathBuf::from("/tmp/fake"),
            branches: branch_names.iter().map(|n| make_branch(n)).collect(),
        }
    }

    fn empty_stacks() -> StackInfo {
        StackInfo {
            stacks: vec![],
            standalone: vec![],
            branch_to_parent: HashMap::new(),
            stale_branches: HashSet::new(),
            detection_status: crate::stack::StackDetectionStatus::Ready,
        }
    }

    #[test]
    fn display_entries_no_stacks_flat_list() {
        let repo = make_repo(&["feat-a", "feat-b", "main"]);
        let stacks = empty_stacks();
        let entries = build_display_entries(&repo, &stacks, &[], &HashSet::new());

        assert!(entries.iter().all(|e| !e.is_header()));
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn display_entries_with_stack_adds_headers() {
        let repo = make_repo(&["auth-base", "auth-ui", "main"]);
        let stacks = StackInfo {
            stacks: vec![Stack {
                name: "auth stack".into(),
                branches: vec!["auth-base".into(), "auth-ui".into()],
            }],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: HashSet::new(),
            detection_status: crate::stack::StackDetectionStatus::Ready,
        };

        let entries = build_display_entries(
            &repo,
            &stacks,
            &[],
            &HashSet::from(["auth stack".to_string()]),
        );

        let headers: Vec<_> = entries.iter().filter(|e| e.is_header()).collect();
        assert_eq!(headers.len(), 2);

        let branches: Vec<_> = entries.iter().filter(|e| !e.is_header()).collect();
        assert_eq!(branches.len(), 3);
    }

    #[test]
    fn display_entries_stack_branches_indented() {
        let repo = make_repo(&["base", "mid", "top", "main"]);
        let stacks = StackInfo {
            stacks: vec![Stack {
                name: "my stack".into(),
                branches: vec!["base".into(), "mid".into(), "top".into()],
            }],
            standalone: vec![],
            branch_to_parent: HashMap::new(),
            stale_branches: HashSet::new(),
            detection_status: crate::stack::StackDetectionStatus::Ready,
        };

        let entries = build_display_entries(
            &repo,
            &stacks,
            &[],
            &HashSet::from(["my stack".to_string()]),
        );

        let branch_entries: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                BranchEntry::Branch { indent, .. } => Some(*indent),
                _ => None,
            })
            .collect();
        assert_eq!(branch_entries[0], 1);
        assert_eq!(branch_entries[1], 2);
        assert_eq!(branch_entries[2], 3);
    }

    #[test]
    fn display_entries_standalone_no_header_when_no_stacks() {
        let repo = make_repo(&["main", "fix"]);
        let stacks = empty_stacks();
        let entries = build_display_entries(&repo, &stacks, &[], &HashSet::new());
        assert!(entries.iter().all(|e| !e.is_header()));
    }

    #[test]
    fn display_entries_use_stack_standalone_order() {
        let repo = make_repo(&["main", "fix", "topic"]);
        let stacks = StackInfo {
            stacks: vec![],
            standalone: vec!["topic".into(), "main".into(), "fix".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: HashSet::new(),
            detection_status: crate::stack::StackDetectionStatus::Ready,
        };

        let entries = build_display_entries(&repo, &stacks, &[], &HashSet::new());
        let names: Vec<_> = entries.iter().map(BranchEntry::branch_name).collect();
        assert_eq!(names, vec!["topic", "main", "fix"]);
    }

    #[test]
    fn branch_for_diff_prefers_stack_parent_over_default_base() {
        let repo = make_repo(&["stack-base", "stack-top", "main"]);
        let mut branch_to_parent = HashMap::new();
        branch_to_parent.insert("stack-top".into(), "stack-base".into());
        let stacks = StackInfo {
            stacks: vec![Stack {
                name: "stack".into(),
                branches: vec!["stack-base".into(), "stack-top".into()],
            }],
            standalone: vec!["main".into()],
            branch_to_parent,
            stale_branches: HashSet::new(),
            detection_status: crate::stack::StackDetectionStatus::Ready,
        };

        let branch = branch_for_diff(&repo, &stacks, "stack-top").unwrap();
        assert_eq!(branch.base_ref.as_deref(), Some("stack-base"));
    }

    #[test]
    fn build_stack_view_includes_parent_child_and_branch_status() {
        let mut repo = make_repo(&["auth-base", "auth-ui", "main"]);
        repo.branches[1].is_head = true;
        repo.branches[1].ahead = 2;
        repo.branches[1].upstream = Some("origin/auth-ui".into());
        let stacks = StackInfo {
            stacks: vec![Stack {
                name: "auth stack".into(),
                branches: vec!["auth-base".into(), "auth-ui".into()],
            }],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::from([
                ("auth-base".into(), "main".into()),
                ("auth-ui".into(), "auth-base".into()),
            ]),
            stale_branches: HashSet::from(["auth-ui".into()]),
            detection_status: crate::stack::StackDetectionStatus::Ready,
        };

        let view = build_stack_view(&repo, &stacks, "auth-ui").unwrap();
        assert_eq!(view.title, "auth stack");
        assert_eq!(view.parent_branch.as_deref(), Some("auth-base"));
        assert_eq!(view.child_branch, None);
        assert_eq!(view.base_ref.as_deref(), Some("auth-base"));
        assert!(view.stale);
        assert_eq!(view.branches.len(), 2);
        assert!(view.branches[1].is_selected);
        assert!(view.branches[1].is_head);
        assert_eq!(view.branches[1].ahead, 2);
    }

    #[test]
    fn build_stack_view_shows_trunk_as_parent_for_bottom_branch() {
        let repo = make_repo(&["auth-base", "auth-ui", "main"]);
        let stacks = StackInfo {
            stacks: vec![Stack {
                name: "auth stack".into(),
                branches: vec!["auth-base".into(), "auth-ui".into()],
            }],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::from([
                ("auth-base".into(), "main".into()),
                ("auth-ui".into(), "auth-base".into()),
            ]),
            stale_branches: HashSet::new(),
            detection_status: crate::stack::StackDetectionStatus::Ready,
        };

        let view = build_stack_view(&repo, &stacks, "auth-base").unwrap();
        assert_eq!(view.parent_branch.as_deref(), Some("main"));
        assert_eq!(view.child_branch.as_deref(), Some("auth-ui"));
    }

    #[test]
    fn diff_preload_targets_follow_visible_branch_order() {
        let repo = make_repo(&["base", "top", "main"]);
        let stacks = StackInfo {
            stacks: vec![Stack {
                name: "stack".into(),
                branches: vec!["base".into(), "top".into()],
            }],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::from([
                ("base".into(), "main".into()),
                ("top".into(), "base".into()),
            ]),
            stale_branches: HashSet::new(),
            detection_status: crate::stack::StackDetectionStatus::Ready,
        };
        let entries =
            build_display_entries(&repo, &stacks, &[], &HashSet::from(["stack".to_string()]));

        let targets = diff_preload_targets(&repo, &stacks, &entries);
        let names: Vec<_> = targets.iter().map(|branch| branch.name.as_str()).collect();
        assert_eq!(names, vec!["base", "top", "main"]);
        assert_eq!(targets[1].base_ref.as_deref(), Some("base"));
    }

    #[test]
    fn display_entries_show_worktree_labels_for_checked_out_branches() {
        let repo = make_repo(&["feature", "main"]);
        let worktrees = vec![
            WorktreeInfo {
                path: PathBuf::from("/tmp/wt-main"),
                branch: Some("main".into()),
                is_bare: false,
                is_active: true,
                is_dirty: false,
            },
            WorktreeInfo {
                path: PathBuf::from("/tmp/wt-feature"),
                branch: Some("feature".into()),
                is_bare: false,
                is_active: false,
                is_dirty: true,
            },
        ];

        let entries = build_display_entries(&repo, &empty_stacks(), &worktrees, &HashSet::new());
        let feature = entries
            .iter()
            .find(|entry| !entry.is_header() && entry.branch_name() == "feature")
            .unwrap();
        match feature {
            BranchEntry::Branch { worktree_label, .. } => {
                assert_eq!(worktree_label.as_deref(), Some("wt-feature"));
            }
            BranchEntry::Header { .. } => panic!("expected branch entry"),
        }
    }
}
