use anyhow::{anyhow, Context, Result};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone)]
pub struct RepoState {
    pub root: PathBuf,
    pub branches: Vec<BranchInfo>,
}

#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
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
    pub lines: Vec<DiffLine>,
    pub file_positions: Vec<usize>,
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
    upstream: Option<String>,
}

pub fn open_repo(path: Option<PathBuf>) -> Result<RepoState> {
    let start = path.unwrap_or(env::current_dir().context("failed to read current directory")?);
    let root = discover_repo_root(&start)?;
    refresh_repo(&root)
}

pub fn refresh_repo(root: &Path) -> Result<RepoState> {
    let raw_branches = local_branches(root)?;
    let default_base = infer_default_base(root, &raw_branches)?;

    let mut branches = Vec::with_capacity(raw_branches.len());
    for raw in raw_branches {
        let base_ref = raw.upstream.clone().or_else(|| default_base.clone());
        let (ahead, behind) = match &raw.upstream {
            Some(upstream) => ahead_behind(root, &raw.name, upstream).unwrap_or((0, 0)),
            None => (0, 0),
        };
        let commit_count = match &base_ref {
            Some(base) => commit_count_above(root, &raw.name, base).unwrap_or(0),
            None => 0,
        };

        branches.push(BranchInfo {
            name: raw.name,
            is_head: raw.is_head,
            upstream: raw.upstream,
            ahead,
            behind,
            commit_count,
            base_ref,
        });
    }

    branches.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(RepoState {
        root: root.to_path_buf(),
        branches,
    })
}

pub fn load_branch_diff(root: &Path, branch: &BranchInfo) -> Result<BranchDiff> {
    let base_ref = branch.base_ref.clone();
    let Some(base_ref_name) = base_ref.clone() else {
        return Ok(BranchDiff {
            branch_name: branch.name.clone(),
            base_ref,
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

    let patch = git(
        root,
        [
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--find-renames",
            merge_base,
            &branch.name,
        ],
    )?;
    let stats = git(root, ["diff", "--numstat", merge_base, &branch.name])?;
    let stat_map = parse_numstat(&stats);

    Ok(parse_diff(
        branch.name.clone(),
        Some(base_ref_name),
        &patch,
        &stat_map,
    ))
}

fn local_branches(root: &Path) -> Result<Vec<RawBranch>> {
    let output = git(
        root,
        [
            "for-each-ref",
            "refs/heads",
            "--format=%(refname:short)\t%(HEAD)\t%(upstream:short)\t%(objectname)",
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
        let _ = parts.next();

        branches.push(RawBranch {
            name,
            is_head: head_marker == "*",
            upstream,
        });
    }

    Ok(branches)
}

fn infer_default_base(root: &Path, branches: &[RawBranch]) -> Result<Option<String>> {
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

fn ahead_behind(root: &Path, branch: &str, upstream: &str) -> Result<(usize, usize)> {
    let output = git(
        root,
        [
            "rev-list",
            "--left-right",
            "--count",
            &format!("{branch}...{upstream}"),
        ],
    )?;
    let mut parts = output.split_whitespace();
    let ahead = parts.next().unwrap_or("0").parse::<usize>().unwrap_or(0);
    let behind = parts.next().unwrap_or("0").parse::<usize>().unwrap_or(0);
    Ok((ahead, behind))
}

fn commit_count_above(root: &Path, branch: &str, base_ref: &str) -> Result<usize> {
    let merge_base = git(root, ["merge-base", branch, base_ref])?;
    let merge_base = merge_base.trim();
    if merge_base.is_empty() {
        return Ok(0);
    }
    let count = git(
        root,
        ["rev-list", "--count", &format!("{merge_base}..{branch}")],
    )?;
    Ok(count.trim().parse::<usize>().unwrap_or(0))
}

fn parse_numstat(output: &str) -> HashMap<String, (String, String)> {
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

fn parse_diff(
    branch_name: String,
    base_ref: Option<String>,
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
            if !emitted_files.contains_key(&path) {
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
                emitted_files.insert(path, true);
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
        lines,
        file_positions,
    }
}

fn git<const N: usize>(root: &Path, args: [&str; N]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to run git {:?}", args))?;

    if !output.status.success() {
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn discover_repo_root(start: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
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
