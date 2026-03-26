use crate::perf;
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
    object_id: String,
    upstream: Option<String>,
    ahead: usize,
    behind: usize,
}

pub fn open_repo(path: Option<PathBuf>) -> Result<RepoState> {
    let start = path.unwrap_or(env::current_dir().context("failed to read current directory")?);
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

pub fn load_branch_diff(root: &Path, branch: &BranchInfo) -> Result<BranchDiff> {
    let _timer = perf::ScopeTimer::new(format!("load_branch_diff branch={}", branch.name));
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

pub(crate) fn parse_diff(
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
    let label = format!("git {}", args.join(" "));
    let _timer = perf::ScopeTimer::new(label);
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let diff = parse_diff("feature".into(), Some("main".into()), "", &HashMap::new());
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

        let diff = parse_diff("feat".into(), Some("main".into()), patch, &stats);
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

        let diff = parse_diff("fix".into(), None, patch, &stats);
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
        let diff = parse_diff("multi".into(), Some("main".into()), patch, &stats);
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
        let diff = parse_diff("bin".into(), None, patch, &stats);
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
        let diff = parse_diff("rename".into(), None, patch, &stats);
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
        let diff = parse_diff("fp".into(), None, patch, &stats);
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
        let diff = parse_diff("nonl".into(), None, patch, &stats);
        let meta_lines: Vec<_> = diff
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Meta)
            .collect();
        assert!(meta_lines.iter().any(|l| l.text.contains("No newline")));
    }
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
