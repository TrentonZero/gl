use crate::{logger, perf};
use anyhow::{anyhow, Context, Result};
use std::{
    collections::hash_map::Entry,
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[derive(Debug, Clone)]
pub struct RepoState {
    pub root: PathBuf,
    pub branches: Vec<BranchInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_active: bool,
    pub is_dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPaths {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub git_common_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub object_id: String,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub commit_count: usize,
    pub base_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BranchDiff {
    pub branch_name: String,
    pub base_ref: Option<String>,
    pub title: Option<String>,
    pub ignore_whitespace: bool,
    pub lines: Vec<DiffLine>,
    pub file_positions: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffOptions {
    pub ignore_whitespace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub oid: String,
    pub short_oid: String,
    pub subject: String,
    pub committed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub oid: String,
    pub short_oid: String,
    pub subject: String,
    pub graph: String,
    pub branch_labels: Vec<String>,
    pub primary_branch: Option<String>,
    pub is_merge: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailKind {
    BranchDiff,
    Status,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffLineKind {
    File,
    Hunk,
    Context,
    Add,
    Del,
    Meta,
}

#[derive(Debug)]
struct RawBranch {
    name: String,
    is_head: bool,
    object_id: String,
    upstream: Option<String>,
    ahead: usize,
    behind: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusEntry {
    staged: char,
    unstaged: char,
    path: String,
}

pub fn open_repo(path: Option<PathBuf>) -> Result<RepoState> {
    let start = path.unwrap_or(env::current_dir().context("failed to read current directory")?);
    if git(&start, ["rev-parse", "--is-bare-repository"])
        .map(|output| output.trim() == "true")
        .unwrap_or(false)
    {
        return Err(anyhow!(
            "bare repositories are not supported; open a working tree instead"
        ));
    }
    let root = discover_repo_root(&start)?;
    refresh_repo(&root)
}

pub fn refresh_repo(root: &Path) -> Result<RepoState> {
    let _timer = perf::ScopeTimer::new("refresh_repo");
    let raw_branches = local_branches(root)?;
    let default_base = infer_default_base(root, &raw_branches)?;

    let mut branches = Vec::with_capacity(raw_branches.len());
    for raw in raw_branches {
        let base_ref = raw.upstream.clone().or_else(|| default_base.clone());

        branches.push(BranchInfo {
            name: raw.name,
            is_head: raw.is_head,
            object_id: raw.object_id,
            upstream: raw.upstream,
            ahead: raw.ahead,
            behind: raw.behind,
            commit_count: 0,
            base_ref,
        });
    }

    branches.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(RepoState {
        root: root.to_path_buf(),
        branches,
    })
}

pub fn repo_paths(root: &Path) -> Result<RepoPaths> {
    Ok(RepoPaths {
        root: root.to_path_buf(),
        git_dir: git_rev_parse_path(root, ["rev-parse", "--path-format=absolute", "--git-dir"])?,
        git_common_dir: git_rev_parse_path(
            root,
            ["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?,
    })
}

pub fn load_commit_counts(root: &Path, repo: &RepoState) -> Vec<(String, usize)> {
    let _timer = perf::ScopeTimer::new(format!(
        "load_commit_counts branches={}",
        repo.branches.len()
    ));
    repo.branches
        .iter()
        .map(|branch| {
            let commit_count = match &branch.base_ref {
                Some(base_ref) if branch.upstream.as_deref() == Some(base_ref.as_str()) => {
                    branch.ahead
                }
                Some(base_ref) => commit_count_above(root, &branch.name, base_ref).unwrap_or(0),
                None => 0,
            };
            (branch.name.clone(), commit_count)
        })
        .collect()
}

pub fn load_branch_diff(
    root: &Path,
    branch: &BranchInfo,
    options: DiffOptions,
) -> Result<BranchDiff> {
    let _timer = perf::ScopeTimer::new(format!("load_branch_diff branch={}", branch.name));
    let base_ref = branch.base_ref.clone();
    let Some(base_ref_name) = base_ref.clone() else {
        return Ok(BranchDiff {
            branch_name: branch.name.clone(),
            base_ref,
            title: None,
            ignore_whitespace: options.ignore_whitespace,
            lines: vec![DiffLine {
                kind: DiffLineKind::Meta,
                text: "No base branch available for diff.".to_string(),
                file_path: None,
            }],
            file_positions: vec![],
        });
    };

    let merge_base = git(root, ["merge-base", &branch.name, &base_ref_name])?;
    let merge_base = merge_base.trim();
    if merge_base.is_empty() {
        return Err(anyhow!(
            "merge-base was empty for {} and {}",
            branch.name,
            base_ref_name
        ));
    }

    let patch = diff_command(
        root,
        &["--find-renames", merge_base, &branch.name],
        options.ignore_whitespace,
    )?;
    let stats = diff_command(
        root,
        &["--numstat", merge_base, &branch.name],
        options.ignore_whitespace,
    )?;
    let stat_map = parse_numstat(&stats);

    Ok(parse_diff(
        branch.name.clone(),
        Some(base_ref_name),
        None,
        options.ignore_whitespace,
        &patch,
        &stat_map,
    ))
}

pub fn load_branch_commits(root: &Path, branch: &BranchInfo) -> Result<Vec<CommitSummary>> {
    let _timer = perf::ScopeTimer::new(format!("load_branch_commits branch={}", branch.name));
    let Some(base_ref_name) = branch.base_ref.as_deref() else {
        return Ok(Vec::new());
    };

    let merge_base = git(root, ["merge-base", &branch.name, base_ref_name])?;
    let merge_base = merge_base.trim();
    if merge_base.is_empty() {
        return Err(anyhow!(
            "merge-base was empty for {} and {}",
            branch.name,
            base_ref_name
        ));
    }

    let log_output = git(
        root,
        [
            "log",
            "--format=%H\t%h\t%cs\t%s",
            &format!("{merge_base}..{}", branch.name),
        ],
    )?;

    Ok(parse_commit_summaries(&log_output))
}

pub fn load_commit_diff(
    root: &Path,
    branch_name: &str,
    commit: &CommitSummary,
    options: DiffOptions,
) -> Result<BranchDiff> {
    let _timer = perf::ScopeTimer::new(format!("load_commit_diff commit={}", commit.short_oid));
    let patch = show_command(
        root,
        &[
            "--format=",
            "--no-color",
            "--no-ext-diff",
            "--find-renames",
            &commit.oid,
        ],
        options.ignore_whitespace,
    )?;
    let stats = show_command(
        root,
        &["--format=", "--numstat", &commit.oid],
        options.ignore_whitespace,
    )?;
    let stat_map = parse_numstat(&stats);

    Ok(parse_diff(
        branch_name.to_string(),
        Some(commit.oid.clone()),
        Some(format!(
            "{} @ {} {}",
            branch_name, commit.short_oid, commit.subject
        )),
        options.ignore_whitespace,
        &patch,
        &stat_map,
    ))
}

pub fn load_working_tree_status(
    root: &Path,
    branch_name: &str,
    options: DiffOptions,
) -> Result<BranchDiff> {
    let _timer = perf::ScopeTimer::new(format!("load_working_tree_status branch={branch_name}"));
    let status_output = git(root, ["status", "--short"])?;
    let status_entries = parse_status_entries(&status_output);
    let staged_patch = diff_command(
        root,
        &["--find-renames", "--cached", "HEAD"],
        options.ignore_whitespace,
    )?;
    let staged_stats = parse_numstat(&diff_command(
        root,
        &["--cached", "--numstat", "HEAD"],
        options.ignore_whitespace,
    )?);
    let unstaged_patch = diff_command(root, &["--find-renames"], options.ignore_whitespace)?;
    let unstaged_stats = parse_numstat(&diff_command(
        root,
        &["--numstat"],
        options.ignore_whitespace,
    )?);

    Ok(build_working_tree_diff(
        branch_name,
        options.ignore_whitespace,
        &status_entries,
        &staged_patch,
        &staged_stats,
        &unstaged_patch,
        &unstaged_stats,
    ))
}

pub fn load_commit_graph(root: &Path, repo: &RepoState) -> Result<Vec<GraphCommit>> {
    let _timer = perf::ScopeTimer::new("load_commit_graph");
    if repo.branches.is_empty() {
        return Ok(Vec::new());
    }

    let mut owner_by_oid = HashMap::new();
    for branch in &repo.branches {
        let rev_list = git(root, ["rev-list", "--first-parent", &branch.name])?;
        for oid in rev_list.lines() {
            owner_by_oid
                .entry(oid.to_string())
                .or_insert_with(|| branch.name.clone());
        }
    }

    let output = git(
        root,
        [
            "log",
            "--topo-order",
            "--first-parent",
            "--format=%H\t%h\t%P\t%s",
            "--branches",
        ],
    )?;
    let labels_by_oid: HashMap<_, Vec<_>> =
        repo.branches
            .iter()
            .fold(HashMap::new(), |mut acc, branch| {
                acc.entry(branch.object_id.clone())
                    .or_insert_with(Vec::new)
                    .push(branch.name.clone());
                acc
            });

    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let oid = parts.next()?.trim().to_string();
            let short_oid = parts.next()?.trim().to_string();
            let parents = parts.next()?.trim().to_string();
            let subject = parts.next()?.trim().to_string();
            if oid.is_empty() || short_oid.is_empty() {
                return None;
            }

            Some(GraphCommit {
                graph: "●".to_string(),
                branch_labels: labels_by_oid.get(&oid).cloned().unwrap_or_default(),
                primary_branch: owner_by_oid.get(&oid).cloned(),
                is_merge: parents.split_whitespace().count() > 1,
                oid,
                short_oid,
                subject,
            })
        })
        .collect())
}

pub fn load_worktrees(active_root: &Path) -> Result<Vec<WorktreeInfo>> {
    let common_dir = repo_paths(active_root)?.git_common_dir;
    let output = git_vec(
        &common_dir,
        &[
            "--git-dir",
            common_dir.to_str().unwrap_or(".git"),
            "worktree",
            "list",
            "--porcelain",
        ],
    )?;
    Ok(parse_worktree_list(&output, active_root))
}

fn parse_worktree_list(output: &str, active_root: &Path) -> Vec<WorktreeInfo> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;
    let mut current_bare = false;

    let flush = |path: &mut Option<PathBuf>,
                 branch: &mut Option<String>,
                 bare: &mut bool,
                 worktrees: &mut Vec<WorktreeInfo>| {
        if let Some(path) = path.take() {
            let is_dirty = !git(&path, ["status", "--short"])
                .map(|status| status.trim().is_empty())
                .unwrap_or(true);
            worktrees.push(WorktreeInfo {
                is_active: path == active_root,
                path,
                branch: branch.take(),
                is_bare: *bare,
                is_dirty,
            });
        }
        *bare = false;
    };

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            flush(
                &mut current_path,
                &mut current_branch,
                &mut current_bare,
                &mut worktrees,
            );
            current_path = Some(PathBuf::from(path.trim()));
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(branch.trim().to_string());
        } else if line == "bare" {
            current_bare = true;
        }
    }

    flush(
        &mut current_path,
        &mut current_branch,
        &mut current_bare,
        &mut worktrees,
    );
    worktrees
}

fn local_branches(root: &Path) -> Result<Vec<RawBranch>> {
    let _timer = perf::ScopeTimer::new("local_branches");
    let output = git(
        root,
        [
            "for-each-ref",
            "refs/heads",
            "--format=%(refname:short)\t%(HEAD)\t%(upstream:short)\t%(upstream:track)\t%(objectname)",
        ],
    )?;

    let mut branches = Vec::new();
    for line in output.lines() {
        let mut parts = line.split('\t');
        let name = parts.next().unwrap_or_default().trim().to_string();
        if name.is_empty() {
            continue;
        }
        let head_marker = parts.next().unwrap_or_default().trim();
        let upstream = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let (ahead, behind) = parts.next().map(parse_upstream_track).unwrap_or((0, 0));
        let object_id = parts.next().unwrap_or_default().trim().to_string();

        branches.push(RawBranch {
            name,
            is_head: head_marker == "*",
            object_id,
            upstream,
            ahead,
            behind,
        });
    }

    Ok(branches)
}

fn infer_default_base(root: &Path, branches: &[RawBranch]) -> Result<Option<String>> {
    let _timer = perf::ScopeTimer::new(format!("infer_default_base branches={}", branches.len()));
    if let Ok(remote_head) = git(
        root,
        ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        let remote_head = remote_head.trim();
        if let Some(default_branch) = remote_head.strip_prefix("origin/") {
            if branches.iter().any(|branch| branch.name == default_branch) {
                return Ok(Some(default_branch.to_string()));
            }
        }
    }

    for candidate in ["main", "master", "trunk"] {
        if branches.iter().any(|branch| branch.name == candidate) {
            return Ok(Some(candidate.to_string()));
        }
    }

    Ok(None)
}

fn parse_upstream_track(track: &str) -> (usize, usize) {
    let trimmed = track.trim().trim_start_matches('[').trim_end_matches(']');
    if trimmed.is_empty() || trimmed == "gone" {
        return (0, 0);
    }

    let mut ahead = 0;
    let mut behind = 0;
    for segment in trimmed.split(',') {
        let segment = segment.trim();
        if let Some(value) = segment.strip_prefix("ahead ") {
            ahead = value.parse::<usize>().unwrap_or(0);
        } else if let Some(value) = segment.strip_prefix("behind ") {
            behind = value.parse::<usize>().unwrap_or(0);
        }
    }

    (ahead, behind)
}

fn parse_status_entries(output: &str) -> Vec<StatusEntry> {
    output
        .lines()
        .filter_map(|line| {
            if line.len() < 3 {
                return None;
            }

            let mut chars = line.chars();
            let staged = chars.next()?;
            let unstaged = chars.next()?;
            let path = line[3..].trim().to_string();
            if path.is_empty() {
                return None;
            }

            Some(StatusEntry {
                staged,
                unstaged,
                path,
            })
        })
        .collect()
}

fn build_working_tree_diff(
    branch_name: &str,
    ignore_whitespace: bool,
    status_entries: &[StatusEntry],
    staged_patch: &str,
    staged_stats: &HashMap<String, (String, String)>,
    unstaged_patch: &str,
    unstaged_stats: &HashMap<String, (String, String)>,
) -> BranchDiff {
    let mut lines = Vec::new();
    let mut file_positions = Vec::new();

    let staged_count = status_entries
        .iter()
        .filter(|entry| !matches!(entry.staged, ' ' | '?'))
        .count();
    let unstaged_count = status_entries
        .iter()
        .filter(|entry| !matches!(entry.unstaged, ' ' | '?'))
        .count();
    let untracked: Vec<_> = status_entries
        .iter()
        .filter(|entry| entry.staged == '?' && entry.unstaged == '?')
        .collect();

    if status_entries.is_empty() {
        lines.push(DiffLine {
            kind: DiffLineKind::Meta,
            text: "Working tree is clean.".to_string(),
            file_path: None,
        });
        return BranchDiff {
            branch_name: branch_name.to_string(),
            base_ref: Some("working tree".to_string()),
            title: None,
            ignore_whitespace,
            lines,
            file_positions,
        };
    }

    lines.push(DiffLine {
        kind: DiffLineKind::Meta,
        text: format!(
            "Working tree status: {staged_count} staged, {unstaged_count} unstaged, {} untracked",
            untracked.len()
        ),
        file_path: None,
    });

    append_status_section(
        &mut lines,
        &mut file_positions,
        "Staged Changes",
        staged_patch,
        staged_stats,
    );
    append_status_section(
        &mut lines,
        &mut file_positions,
        "Unstaged Changes",
        unstaged_patch,
        unstaged_stats,
    );

    if !untracked.is_empty() {
        lines.push(DiffLine {
            kind: DiffLineKind::Meta,
            text: "Untracked Files".to_string(),
            file_path: None,
        });
        for entry in untracked {
            file_positions.push(lines.len());
            lines.push(DiffLine {
                kind: DiffLineKind::File,
                text: format!("── {} ── untracked", entry.path),
                file_path: Some(entry.path.clone()),
            });
        }
    }

    BranchDiff {
        branch_name: branch_name.to_string(),
        base_ref: Some("working tree".to_string()),
        title: None,
        ignore_whitespace,
        lines,
        file_positions,
    }
}

fn append_status_section(
    lines: &mut Vec<DiffLine>,
    file_positions: &mut Vec<usize>,
    title: &str,
    patch: &str,
    stat_map: &HashMap<String, (String, String)>,
) {
    if patch.trim().is_empty() {
        return;
    }

    lines.push(DiffLine {
        kind: DiffLineKind::Meta,
        text: title.to_string(),
        file_path: None,
    });

    let section = parse_diff(String::new(), None, None, false, patch, stat_map);
    let offset = lines.len();
    file_positions.extend(
        section
            .file_positions
            .into_iter()
            .map(|position| position + offset),
    );
    lines.extend(section.lines);
}

fn commit_count_above(root: &Path, branch: &str, base_ref: &str) -> Result<usize> {
    let _timer = perf::ScopeTimer::new(format!(
        "commit_count_above branch={branch} base={base_ref}"
    ));
    let count = git(root, ["rev-list", "--count", branch, "--not", base_ref])?;
    Ok(count.trim().parse::<usize>().unwrap_or(0))
}

pub(crate) fn parse_numstat(output: &str) -> HashMap<String, (String, String)> {
    let mut stats = HashMap::new();
    for line in output.lines() {
        let mut parts = line.split('\t');
        let added = parts.next().unwrap_or_default().to_string();
        let removed = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        if !path.is_empty() {
            stats.insert(path, (added, removed));
        }
    }
    stats
}

fn parse_commit_summaries(output: &str) -> Vec<CommitSummary> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let oid = parts.next()?.trim();
            let short_oid = parts.next()?.trim();
            let committed_at = parts.next()?.trim();
            let subject = parts.next()?.trim();
            if oid.is_empty() || short_oid.is_empty() || subject.is_empty() {
                return None;
            }

            Some(CommitSummary {
                oid: oid.to_string(),
                short_oid: short_oid.to_string(),
                subject: subject.to_string(),
                committed_at: committed_at.to_string(),
            })
        })
        .collect()
}

pub(crate) fn parse_diff(
    branch_name: String,
    base_ref: Option<String>,
    title: Option<String>,
    ignore_whitespace: bool,
    patch: &str,
    stat_map: &HashMap<String, (String, String)>,
) -> BranchDiff {
    let mut lines = Vec::new();
    let mut file_positions = Vec::new();
    let mut current_path: Option<String> = None;
    let mut emitted_files = HashMap::<String, bool>::new();

    if patch.trim().is_empty() {
        lines.push(DiffLine {
            kind: DiffLineKind::Meta,
            text: "Branch is identical to its base.".to_string(),
            file_path: None,
        });
    }

    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            let path = rest.split(" b/").next().unwrap_or(rest).trim().to_string();
            current_path = Some(path.clone());
            if let Entry::Vacant(entry) = emitted_files.entry(path.clone()) {
                let (added, removed) = stat_map
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| ("0".to_string(), "0".to_string()));
                file_positions.push(lines.len());
                lines.push(DiffLine {
                    kind: DiffLineKind::File,
                    text: format!("── {path} ── +{added} -{removed}"),
                    file_path: Some(path.clone()),
                });
                entry.insert(true);
            }
            continue;
        }

        if line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("new file mode ")
            || line.starts_with("deleted file mode ")
            || line.starts_with("similarity index ")
            || line.starts_with("rename from ")
            || line.starts_with("rename to ")
        {
            continue;
        }

        let (kind, text) = if line.starts_with("@@") {
            (DiffLineKind::Hunk, line.to_string())
        } else if line.starts_with('+') {
            (DiffLineKind::Add, line.to_string())
        } else if line.starts_with('-') {
            (DiffLineKind::Del, line.to_string())
        } else if line.starts_with('\\') {
            (DiffLineKind::Meta, line.to_string())
        } else if line.starts_with("Binary files ") {
            let path = current_path
                .clone()
                .unwrap_or_else(|| "binary file".to_string());
            (DiffLineKind::Meta, format!("{path}: binary file changed"))
        } else {
            (DiffLineKind::Context, format!(" {line}"))
        };

        lines.push(DiffLine {
            kind,
            text,
            file_path: current_path.clone(),
        });
    }

    BranchDiff {
        branch_name,
        base_ref,
        title,
        ignore_whitespace,
        lines,
        file_positions,
    }
}

fn diff_command(root: &Path, args: &[&str], ignore_whitespace: bool) -> Result<String> {
    let mut owned = vec!["diff", "--no-color", "--no-ext-diff"];
    if ignore_whitespace {
        owned.push("--ignore-all-space");
    }
    owned.extend_from_slice(args);
    git_vec(root, &owned)
}

fn show_command(root: &Path, args: &[&str], ignore_whitespace: bool) -> Result<String> {
    let mut owned = vec!["show"];
    if ignore_whitespace {
        owned.push("--ignore-all-space");
    }
    owned.extend_from_slice(args);
    git_vec(root, &owned)
}

fn git<const N: usize>(root: &Path, args: [&str; N]) -> Result<String> {
    let label = format!("git {}", args.join(" "));
    let _timer = perf::ScopeTimer::new(label);
    let output = run_git_command(root, &args)?;

    if !output.status.success() {
        logger::warn(format!(
            "git {:?} failed in {}: {}",
            args,
            root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_vec(root: &Path, args: &[&str]) -> Result<String> {
    let label = format!("git {}", args.join(" "));
    let _timer = perf::ScopeTimer::new(label);
    let output = run_git_command(root, args)?;

    if !output.status.success() {
        logger::warn(format!(
            "git {:?} failed in {}: {}",
            args,
            root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_rev_parse_path<const N: usize>(root: &Path, args: [&str; N]) -> Result<PathBuf> {
    let output = git(root, args)?;
    let path = output.trim();
    if path.is_empty() {
        return Err(anyhow!("git rev-parse returned an empty path"));
    }
    Ok(PathBuf::from(path))
}

fn run_git_command(root: &Path, args: &[&str]) -> Result<Output> {
    for binary in ["git", "/usr/bin/git"] {
        match Command::new(binary).args(args).current_dir(root).output() {
            Ok(output) => return Ok(output),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                logger::warn(format!(
                    "failed to run git {:?} in {}: {}",
                    args,
                    root.display(),
                    error
                ));
                return Err(error).with_context(|| format!("failed to run git {:?}", args));
            }
        }
    }

    logger::error(format!("failed to find git executable for {:?}", args));
    Err(anyhow!("failed to find git executable"))
        .with_context(|| format!("failed to run git {:?}", args))
}

fn discover_repo_root(start: &Path) -> Result<PathBuf> {
    let output = run_git_command(start, &["rev-parse", "--show-toplevel"])
        .context("failed to run git rev-parse --show-toplevel")?;

    if !output.status.success() {
        return Err(anyhow!(
            "failed to discover git repository: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err(anyhow!("git rev-parse returned an empty repository path"));
    }

    Ok(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("gl-git-{label}-{}-{nanos}", std::process::id()))
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .or_else(|_| {
                Command::new("/usr/bin/git")
                    .args(args)
                    .current_dir(root)
                    .output()
            })
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn parse_numstat_basic() {
        let input = "10\t5\tsrc/main.rs\n3\t1\tsrc/lib.rs\n";
        let stats = parse_numstat(input);
        assert_eq!(stats.len(), 2);
        assert_eq!(
            stats.get("src/main.rs"),
            Some(&("10".to_string(), "5".to_string()))
        );
        assert_eq!(
            stats.get("src/lib.rs"),
            Some(&("3".to_string(), "1".to_string()))
        );
    }

    #[test]
    fn parse_numstat_empty() {
        assert!(parse_numstat("").is_empty());
    }

    #[test]
    fn parse_upstream_track_ahead_and_behind() {
        assert_eq!(parse_upstream_track("[ahead 3, behind 2]"), (3, 2));
    }

    #[test]
    fn parse_upstream_track_single_direction() {
        assert_eq!(parse_upstream_track("[ahead 4]"), (4, 0));
        assert_eq!(parse_upstream_track("[behind 5]"), (0, 5));
    }

    #[test]
    fn parse_upstream_track_empty_or_gone() {
        assert_eq!(parse_upstream_track(""), (0, 0));
        assert_eq!(parse_upstream_track("[gone]"), (0, 0));
    }

    #[test]
    fn parse_status_entries_reads_porcelain_short_codes() {
        let entries = parse_status_entries("M  src/main.rs\n M src/ui.rs\n?? notes.txt\n");
        assert_eq!(
            entries,
            vec![
                StatusEntry {
                    staged: 'M',
                    unstaged: ' ',
                    path: "src/main.rs".into(),
                },
                StatusEntry {
                    staged: ' ',
                    unstaged: 'M',
                    path: "src/ui.rs".into(),
                },
                StatusEntry {
                    staged: '?',
                    unstaged: '?',
                    path: "notes.txt".into(),
                },
            ]
        );
    }

    #[test]
    fn parse_numstat_binary_file() {
        let input = "-\t-\timage.png\n";
        let stats = parse_numstat(input);
        assert_eq!(
            stats.get("image.png"),
            Some(&("-".to_string(), "-".to_string()))
        );
    }

    #[test]
    fn parse_diff_empty_patch() {
        let diff = parse_diff(
            "feature".into(),
            Some("main".into()),
            None,
            false,
            "",
            &HashMap::new(),
        );
        assert_eq!(diff.branch_name, "feature");
        assert_eq!(diff.base_ref.as_deref(), Some("main"));
        assert_eq!(diff.lines.len(), 1);
        assert_eq!(diff.lines[0].kind, DiffLineKind::Meta);
        assert!(diff.lines[0].text.contains("identical"));
    }

    #[test]
    fn parse_diff_single_file_add() {
        let patch = "\
diff --git a/hello.txt b/hello.txt
new file mode 100644
index 0000000..ce01362
--- /dev/null
+++ b/hello.txt
@@ -0,0 +1,2 @@
+hello
+world";
        let mut stats = HashMap::new();
        stats.insert("hello.txt".to_string(), ("2".to_string(), "0".to_string()));

        let diff = parse_diff(
            "feat".into(),
            Some("main".into()),
            None,
            false,
            patch,
            &stats,
        );
        assert_eq!(diff.branch_name, "feat");
        assert_eq!(diff.file_positions, vec![0]);

        // File header
        assert_eq!(diff.lines[0].kind, DiffLineKind::File);
        assert!(diff.lines[0].text.contains("hello.txt"));
        assert!(diff.lines[0].text.contains("+2"));

        // Hunk header
        assert_eq!(diff.lines[1].kind, DiffLineKind::Hunk);

        // Added lines
        assert_eq!(diff.lines[2].kind, DiffLineKind::Add);
        assert_eq!(diff.lines[2].text, "+hello");
        assert_eq!(diff.lines[3].kind, DiffLineKind::Add);
        assert_eq!(diff.lines[3].text, "+world");
    }

    #[test]
    fn parse_diff_modification() {
        let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
index abc1234..def5678 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn main() {
-    println!(\"hello\");
+    println!(\"world\");
 }";
        let mut stats = HashMap::new();
        stats.insert("src/lib.rs".to_string(), ("1".to_string(), "1".to_string()));

        let diff = parse_diff("fix".into(), None, None, false, patch, &stats);
        assert_eq!(diff.file_positions, vec![0]);
        assert_eq!(diff.lines[0].kind, DiffLineKind::File);
        assert!(diff.lines[0].text.contains("src/lib.rs"));

        // Context, del, add, context
        assert_eq!(diff.lines[2].kind, DiffLineKind::Context);
        assert_eq!(diff.lines[3].kind, DiffLineKind::Del);
        assert_eq!(diff.lines[4].kind, DiffLineKind::Add);
        assert_eq!(diff.lines[5].kind, DiffLineKind::Context);
    }

    #[test]
    fn parse_diff_multiple_files() {
        let patch = "\
diff --git a/a.txt b/a.txt
index 1111..2222 100644
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-old
+new
diff --git a/b.txt b/b.txt
index 3333..4444 100644
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-foo
+bar";
        let stats = HashMap::new();
        let diff = parse_diff(
            "multi".into(),
            Some("main".into()),
            None,
            false,
            patch,
            &stats,
        );
        assert_eq!(diff.file_positions.len(), 2);
        assert_eq!(diff.lines[diff.file_positions[0]].kind, DiffLineKind::File);
        assert!(diff.lines[diff.file_positions[0]].text.contains("a.txt"));
        assert_eq!(diff.lines[diff.file_positions[1]].kind, DiffLineKind::File);
        assert!(diff.lines[diff.file_positions[1]].text.contains("b.txt"));
    }

    #[test]
    fn parse_diff_binary_file() {
        let patch = "\
diff --git a/image.png b/image.png
index 1111..2222 100644
Binary files a/image.png and b/image.png differ";
        let stats = HashMap::new();
        let diff = parse_diff("bin".into(), None, None, false, patch, &stats);
        assert!(diff
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Meta && l.text.contains("binary")));
    }

    #[test]
    fn parse_diff_rename() {
        let patch = "\
diff --git a/old_name.rs b/new_name.rs
similarity index 95%
rename from old_name.rs
rename to new_name.rs
index 1111..2222 100644
--- a/old_name.rs
+++ b/new_name.rs
@@ -1 +1 @@
-old
+new";
        let stats = HashMap::new();
        let diff = parse_diff("rename".into(), None, None, false, patch, &stats);
        // rename from/to lines should be skipped, file header emitted
        assert_eq!(diff.file_positions.len(), 1);
        assert_eq!(diff.lines[0].kind, DiffLineKind::File);
    }

    #[test]
    fn parse_diff_file_path_on_lines() {
        let patch = "\
diff --git a/foo.rs b/foo.rs
index 1111..2222 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1 +1 @@
-old
+new";
        let stats = HashMap::new();
        let diff = parse_diff("fp".into(), None, None, false, patch, &stats);
        // Code lines should have file_path set
        for line in &diff.lines {
            if matches!(line.kind, DiffLineKind::Add | DiffLineKind::Del) {
                assert_eq!(line.file_path.as_deref(), Some("foo.rs"));
            }
        }
    }

    #[test]
    fn parse_diff_no_newline_marker() {
        let patch = "\
diff --git a/a.txt b/a.txt
index 1111..2222 100644
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-old
+new
\\ No newline at end of file";
        let stats = HashMap::new();
        let diff = parse_diff("nonl".into(), None, None, false, patch, &stats);
        let meta_lines: Vec<_> = diff
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Meta)
            .collect();
        assert!(meta_lines.iter().any(|l| l.text.contains("No newline")));
    }

    #[test]
    fn build_working_tree_diff_reports_clean_tree() {
        let diff =
            build_working_tree_diff("main", false, &[], "", &HashMap::new(), "", &HashMap::new());
        assert_eq!(diff.base_ref.as_deref(), Some("working tree"));
        assert_eq!(diff.lines.len(), 1);
        assert!(diff.lines[0].text.contains("clean"));
    }

    #[test]
    fn load_working_tree_status_captures_staged_unstaged_and_untracked_changes() {
        let repo_root = unique_temp_dir("status-view");
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init", "-b", "main"]);
        run_git(&repo_root, &["config", "user.name", "GL Test"]);
        run_git(&repo_root, &["config", "user.email", "gl@example.com"]);

        fs::write(repo_root.join("staged.txt"), "before\n").unwrap();
        fs::write(repo_root.join("unstaged.txt"), "before\n").unwrap();
        run_git(&repo_root, &["add", "staged.txt", "unstaged.txt"]);
        run_git(&repo_root, &["commit", "-m", "initial"]);

        fs::write(repo_root.join("staged.txt"), "after staged\n").unwrap();
        fs::write(repo_root.join("unstaged.txt"), "after unstaged\n").unwrap();
        fs::write(repo_root.join("untracked.txt"), "brand new\n").unwrap();
        run_git(&repo_root, &["add", "staged.txt"]);

        let diff = load_working_tree_status(
            &repo_root,
            "main",
            DiffOptions {
                ignore_whitespace: false,
            },
        )
        .unwrap();
        let text: Vec<_> = diff.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(diff.base_ref.as_deref(), Some("working tree"));
        assert!(text
            .iter()
            .any(|line| line.contains("1 staged, 1 unstaged, 1 untracked")));
        assert!(text.iter().any(|line| line.contains("Staged Changes")));
        assert!(text.iter().any(|line| line.contains("Unstaged Changes")));
        assert!(text.iter().any(|line| line.contains("Untracked Files")));
        assert!(text.iter().any(|line| line.contains("staged.txt")));
        assert!(text.iter().any(|line| line.contains("unstaged.txt")));
        assert!(text.iter().any(|line| line.contains("untracked.txt")));

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn load_branch_commits_and_commit_diff_follow_branch_history() {
        let repo_root = unique_temp_dir("commit-breakdown");
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init", "-b", "main"]);
        run_git(&repo_root, &["config", "user.name", "GL Test"]);
        run_git(&repo_root, &["config", "user.email", "gl@example.com"]);

        fs::write(repo_root.join("notes.txt"), "base\n").unwrap();
        run_git(&repo_root, &["add", "notes.txt"]);
        run_git(&repo_root, &["commit", "-m", "initial"]);

        run_git(&repo_root, &["checkout", "-b", "feature"]);
        fs::write(repo_root.join("notes.txt"), "base\nfirst\n").unwrap();
        run_git(&repo_root, &["commit", "-am", "first change"]);
        fs::write(repo_root.join("notes.txt"), "base\nfirst\nsecond\n").unwrap();
        run_git(&repo_root, &["commit", "-am", "second change"]);

        let repo = refresh_repo(&repo_root).unwrap();
        let branch = repo
            .branches
            .iter()
            .find(|branch| branch.name == "feature")
            .unwrap();

        let commits = load_branch_commits(&repo_root, branch).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "second change");
        assert_eq!(commits[1].subject, "first change");

        let diff = load_commit_diff(
            &repo_root,
            "feature",
            &commits[0],
            DiffOptions {
                ignore_whitespace: false,
            },
        )
        .unwrap();
        assert_eq!(diff.branch_name, "feature");
        assert!(diff
            .title
            .as_deref()
            .is_some_and(|title| title.contains("second change")));
        assert!(diff
            .lines
            .iter()
            .any(|line| line.text.contains("notes.txt")));

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn load_branch_diff_can_ignore_whitespace_only_changes() {
        let repo_root = unique_temp_dir("whitespace-diff");
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init", "-b", "main"]);
        run_git(&repo_root, &["config", "user.name", "GL Test"]);
        run_git(&repo_root, &["config", "user.email", "gl@example.com"]);

        fs::write(
            repo_root.join("main.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();
        run_git(&repo_root, &["add", "main.rs"]);
        run_git(&repo_root, &["commit", "-m", "initial"]);
        run_git(&repo_root, &["checkout", "-b", "feature"]);
        fs::write(
            repo_root.join("main.rs"),
            "fn main(){\n println!(\"hi\");\n}\n",
        )
        .unwrap();
        run_git(&repo_root, &["commit", "-am", "whitespace only"]);

        let repo = refresh_repo(&repo_root).unwrap();
        let branch = repo
            .branches
            .iter()
            .find(|branch| branch.name == "feature")
            .unwrap();
        let regular = load_branch_diff(
            &repo_root,
            branch,
            DiffOptions {
                ignore_whitespace: false,
            },
        )
        .unwrap();
        let ignored = load_branch_diff(
            &repo_root,
            branch,
            DiffOptions {
                ignore_whitespace: true,
            },
        )
        .unwrap();

        assert!(regular
            .lines
            .iter()
            .any(|line| matches!(line.kind, DiffLineKind::Add | DiffLineKind::Del)));
        assert!(ignored
            .lines
            .iter()
            .any(|line| line.text.contains("identical")));
        assert!(ignored.ignore_whitespace);

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn repo_paths_resolve_git_and_common_dirs_for_standard_repo() {
        let repo_root = unique_temp_dir("repo-paths");
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init", "-b", "main"]);

        let paths = repo_paths(&repo_root).unwrap();
        assert_eq!(paths.root, repo_root);
        assert!(paths.git_dir.ends_with(".git"));
        assert_eq!(paths.git_dir, paths.git_common_dir);

        fs::remove_dir_all(paths.root).unwrap();
    }

    #[test]
    fn open_repo_rejects_bare_repositories() {
        let repo_root = unique_temp_dir("bare-repo");
        run_git(
            Path::new(std::env::temp_dir().as_path()),
            &["init", "--bare", repo_root.to_str().unwrap()],
        );

        let error = open_repo(Some(repo_root.clone())).unwrap_err().to_string();
        assert!(error.contains("bare repositories are not supported"));

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn load_commit_graph_marks_branch_heads_and_owners() {
        let repo_root = unique_temp_dir("graph-view");
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init", "-b", "main"]);
        run_git(&repo_root, &["config", "user.name", "GL Test"]);
        run_git(&repo_root, &["config", "user.email", "gl@example.com"]);

        fs::write(repo_root.join("notes.txt"), "base\n").unwrap();
        run_git(&repo_root, &["add", "notes.txt"]);
        run_git(&repo_root, &["commit", "-m", "initial"]);
        run_git(&repo_root, &["checkout", "-b", "feature"]);
        fs::write(repo_root.join("notes.txt"), "base\nfeature\n").unwrap();
        run_git(&repo_root, &["commit", "-am", "feature change"]);

        let repo = refresh_repo(&repo_root).unwrap();
        let graph = load_commit_graph(&repo_root, &repo).unwrap();
        assert!(!graph.is_empty());
        assert!(graph
            .iter()
            .any(|commit| commit.branch_labels.iter().any(|label| label == "feature")));
        assert!(graph
            .iter()
            .any(|commit| commit.primary_branch.as_deref() == Some("feature")));

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn parse_worktree_list_reads_branch_active_and_dirty_flags() {
        let root = unique_temp_dir("parse-worktree-list");
        let other = root.with_extension("other");
        let output = format!(
            "worktree {}\nHEAD deadbeef\nbranch refs/heads/main\n\nworktree {}\nHEAD feedface\nbranch refs/heads/feature\n",
            root.display(),
            other.display()
        );

        let worktrees = parse_worktree_list(&output, &root);
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[0].branch.as_deref(), Some("main"));
        assert!(worktrees[0].is_active);
        assert_eq!(worktrees[1].branch.as_deref(), Some("feature"));
    }
}
