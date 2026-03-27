mod config;
mod git;
mod perf;
mod stack;
mod syntax;
mod ui;
mod watch;

use anyhow::{Context, Result};
use config::{AppConfig, ColorScheme, DiffViewMode};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use git::{
    load_branch_commits, load_branch_diff, load_commit_counts, load_commit_diff, load_commit_graph,
    load_working_tree_status, load_worktrees, open_repo, refresh_repo, BranchDiff, BranchInfo,
    CommitSummary, DetailKind, DiffOptions, GraphCommit, RepoState, WorktreeInfo,
};
use ratatui::{backend::CrosstermBackend, text::Line, Terminal};
use stack::{detect_stacks, enrich_stacks, StackDetectionStatus, StackInfo};
use std::{
    collections::{HashMap, HashSet},
    env, io, mem,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};
use syntax::SyntaxHighlighter;
use ui::{
    draw, BranchEntry, DrawState, FocusedPane, GraphView, StackView, StackViewBranch, WorktreeView,
};
use watch::{start_repo_watcher, RepoWatcher, WatchMessage};

#[cfg(test)]
static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn main() -> Result<()> {
    let _main_timer = perf::ScopeTimer::new("main");
    let cli = parse_cli_args(env::args().skip(1).collect())?;
    if cli.show_help {
        print_help();
        return Ok(());
    }
    if cli.show_version {
        println!("gl {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut config = {
        let _timer = perf::ScopeTimer::new("AppConfig::load");
        AppConfig::load()
    };
    if let Some(color_scheme) = cli.color_scheme {
        config.color_scheme = color_scheme;
    }
    let repo = {
        let _timer = perf::ScopeTimer::new("open_repo");
        open_repo(cli.repo_path)?
    };
    let stack_info = load_initial_stack_info(&repo);
    let mut app = {
        let _timer = perf::ScopeTimer::new("App::new");
        App::new(config, repo, stack_info)
    };
    app.run()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CliArgs {
    repo_path: Option<PathBuf>,
    show_help: bool,
    show_version: bool,
    color_scheme: Option<ColorScheme>,
}

fn parse_cli_args(args: Vec<String>) -> Result<CliArgs> {
    let mut cli = CliArgs::default();
    let mut positional = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => cli.show_help = true,
            "--version" | "-V" => cli.show_version = true,
            "--color-scheme" => {
                let Some(value) = args.next() else {
                    anyhow::bail!(
                        "missing value for --color-scheme; supported values: {}",
                        supported_color_schemes()
                    );
                };
                cli.color_scheme = Some(parse_color_scheme_arg(&value)?);
            }
            _ if arg.starts_with("--color-scheme=") => {
                let value = arg.trim_start_matches("--color-scheme=");
                cli.color_scheme = Some(parse_color_scheme_arg(value)?);
            }
            _ if arg.starts_with('-') => anyhow::bail!("unknown argument `{arg}`; try `gl --help`"),
            _ => positional.push(arg),
        }
    }
    if positional.len() > 1 {
        anyhow::bail!("expected at most one repository path; try `gl --help`");
    }
    cli.repo_path = positional.into_iter().next().map(PathBuf::from);
    Ok(cli)
}

fn print_help() {
    println!("{}", help_text());
}

fn help_text() -> String {
    format!(
        "\
gl {version}

USAGE:
  gl
  gl <path>
  gl --color-scheme <scheme>
  gl --help
  gl --version

OPTIONS:
  -h, --help       Show this help output
  -V, --version    Show the application version
      --color-scheme <scheme>
                   Override the configured accent color
                   Supported: {schemes}
",
        version = env!("CARGO_PKG_VERSION"),
        schemes = supported_color_schemes()
    )
}

fn parse_color_scheme_arg(value: &str) -> Result<ColorScheme> {
    ColorScheme::parse(value).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown color scheme `{value}`; supported values: {}",
            supported_color_schemes()
        )
    })
}

fn supported_color_schemes() -> String {
    ColorScheme::ALL
        .iter()
        .map(|scheme| scheme.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

struct StackLoadResult {
    request_id: usize,
    stack_info: StackInfo,
}

struct CommitCountLoadResult {
    request_id: usize,
    commit_counts: Vec<(String, usize)>,
}

#[derive(Clone)]
struct PreloadedBranchDiff {
    diff: BranchDiff,
    highlighted_diff: Vec<Line<'static>>,
}

struct DiffPreloadResult {
    request_id: usize,
    branch_name: String,
    preloaded: Result<PreloadedBranchDiff, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BranchDetailDiffMode {
    Branch,
    Commit { selected_index: usize },
}

#[derive(Debug, Clone)]
struct BranchDetailState {
    branch_name: String,
    commits: Vec<CommitSummary>,
    commit_list_open: bool,
    commit_list_selected: usize,
    diff_mode: BranchDetailDiffMode,
    info_overlay: Option<Vec<String>>,
}

struct App {
    config: AppConfig,
    repo: RepoState,
    stack_info: StackInfo,
    expanded_stacks: HashSet<String>,
    manual_stack_states: HashMap<String, bool>,
    auto_expanded_stack: Option<String>,
    display_entries: Vec<BranchEntry>,
    selected_index: usize,
    show_stack_view: bool,
    show_graph_view: bool,
    show_worktree_view: bool,
    detail_kind: Option<DetailKind>,
    branch_diff: Option<BranchDiff>,
    highlighted_diff: Option<Vec<Line<'static>>>,
    diff_scroll: usize,
    show_help: bool,
    focus: FocusedPane,
    search_mode: bool,
    search_input: String,
    search_matches: Vec<usize>,
    search_cursor: usize,
    command_mode: bool,
    command_input: String,
    stack_result_tx: Sender<StackLoadResult>,
    stack_result_rx: Receiver<StackLoadResult>,
    stack_request_id: usize,
    commit_count_result_tx: Sender<CommitCountLoadResult>,
    commit_count_result_rx: Receiver<CommitCountLoadResult>,
    commit_count_request_id: usize,
    diff_preload_result_tx: Sender<DiffPreloadResult>,
    diff_preload_result_rx: Receiver<DiffPreloadResult>,
    diff_preload_request_id: usize,
    diff_preload_started: bool,
    preloaded_diffs: HashMap<String, PreloadedBranchDiff>,
    diff_preload_failures: HashMap<String, String>,
    pending_diff_preloads: HashSet<String>,
    branch_detail: Option<BranchDetailState>,
    graph_commits: Vec<GraphCommit>,
    graph_selected_index: usize,
    worktrees: Vec<WorktreeInfo>,
    worktree_selected_index: usize,
    diff_view: DiffViewMode,
    ignore_whitespace: bool,
    _watch_event_tx: Sender<WatchMessage>,
    watch_event_rx: Receiver<WatchMessage>,
    _repo_watcher: Option<RepoWatcher>,
    watch_notice: Option<String>,
    user_notice: Option<String>,
}

impl App {
    fn new(config: AppConfig, repo: RepoState, stack_info: StackInfo) -> Self {
        Self::new_with_watcher(config, repo, stack_info, true)
    }

    fn new_with_watcher(
        config: AppConfig,
        repo: RepoState,
        stack_info: StackInfo,
        start_watcher: bool,
    ) -> Self {
        let _ = &config.worktree_path_defaults;
        let diff_view = config.diff_view;
        let ignore_whitespace = config.ignore_whitespace;
        let worktrees = load_worktrees(&repo.root).unwrap_or_default();
        let expanded_stacks = initial_expanded_stacks(&repo, &stack_info);
        let display_entries =
            build_display_entries(&repo, &stack_info, &worktrees, &expanded_stacks);
        let (stack_result_tx, stack_result_rx) = mpsc::channel();
        let (commit_count_result_tx, commit_count_result_rx) = mpsc::channel();
        let (diff_preload_result_tx, diff_preload_result_rx) = mpsc::channel();
        let (watch_event_tx, watch_event_rx) = mpsc::channel();
        let (repo_watcher, watch_notice) = if start_watcher {
            match start_repo_watcher(&repo.root, watch_event_tx.clone()) {
                Ok(watcher) => (Some(watcher), None),
                Err(error) => (
                    None,
                    Some(format!(
                        "Filesystem watching unavailable; use R to refresh ({error})"
                    )),
                ),
            }
        } else {
            (None, None)
        };
        let mut app = Self {
            config,
            repo,
            stack_info,
            expanded_stacks,
            manual_stack_states: HashMap::new(),
            auto_expanded_stack: None,
            display_entries,
            selected_index: 0,
            show_stack_view: false,
            show_graph_view: false,
            show_worktree_view: false,
            detail_kind: None,
            branch_diff: None,
            highlighted_diff: None,
            diff_scroll: 0,
            show_help: false,
            focus: FocusedPane::BranchList,
            search_mode: false,
            search_input: String::new(),
            search_matches: Vec::new(),
            search_cursor: 0,
            command_mode: false,
            command_input: String::new(),
            stack_result_tx,
            stack_result_rx,
            stack_request_id: 0,
            commit_count_result_tx,
            commit_count_result_rx,
            commit_count_request_id: 0,
            diff_preload_result_tx,
            diff_preload_result_rx,
            diff_preload_request_id: 0,
            diff_preload_started: false,
            preloaded_diffs: HashMap::new(),
            diff_preload_failures: HashMap::new(),
            pending_diff_preloads: HashSet::new(),
            branch_detail: None,
            graph_commits: Vec::new(),
            graph_selected_index: 0,
            worktrees,
            worktree_selected_index: 0,
            diff_view,
            ignore_whitespace,
            _watch_event_tx: watch_event_tx,
            watch_event_rx,
            _repo_watcher: repo_watcher,
            watch_notice,
            user_notice: None,
        };
        app.reload_stack_decorations_async();
        app.reload_commit_counts_async();
        app
    }

    #[cfg(test)]
    fn new_for_test(config: AppConfig, repo: RepoState, stack_info: StackInfo) -> Self {
        Self::new_with_watcher(config, repo, stack_info, false)
    }

    fn run(&mut self) -> Result<()> {
        let _timer = perf::ScopeTimer::new("App::run");
        let mut stdout = io::stdout();
        enable_raw_mode().context("failed to enable raw mode")?;
        execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;
        terminal.clear().ok();

        let result = self.event_loop(&mut terminal);
        self.restore_terminal(&mut terminal)?;
        result
    }

    fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let mut needs_redraw = true;
        let mut first_draw_logged = false;
        loop {
            if self.apply_pending_watch_events()? {
                needs_redraw = true;
            }
            if self.apply_pending_stack_updates() {
                needs_redraw = true;
            }
            if self.apply_pending_commit_count_updates() {
                needs_redraw = true;
            }
            if self.apply_pending_diff_preloads() {
                needs_redraw = true;
            }

            if needs_redraw {
                let _draw_timer = if !first_draw_logged {
                    Some(perf::ScopeTimer::new("first terminal draw"))
                } else {
                    None
                };
                terminal.draw(|frame| {
                    draw(
                        frame,
                        DrawState {
                            config: &self.config,
                            repo: &self.repo,
                            display_entries: &self.display_entries,
                            selected_index: self.selected_index,
                            stack_view: self.current_stack_view().as_ref(),
                            graph_view: self.current_graph_view(),
                            worktree_view: self.current_worktree_view(),
                            detail_kind: self.detail_kind,
                            branch_diff: self.branch_diff.as_ref(),
                            highlighted_diff: self.highlighted_diff.as_deref(),
                            diff_scroll: self.diff_scroll,
                            diff_view: self.diff_view,
                            show_help: self.show_help,
                            focus: self.focus,
                            commit_list_overlay: self.commit_list_overlay_items(),
                            commit_list_selected: self.commit_list_overlay_selected(),
                            info_overlay: self
                                .branch_detail
                                .as_ref()
                                .and_then(|detail| detail.info_overlay.as_deref()),
                            search: if self.search_mode {
                                Some(self.search_input.as_str())
                            } else {
                                None
                            },
                            notice: self.current_notice(),
                            command_input: self.command_mode.then_some(self.command_input.as_str()),
                        },
                    );
                })?;
                if !first_draw_logged {
                    perf::log("first frame rendered");
                    first_draw_logged = true;
                    self.start_diff_preload_async();
                }
                needs_redraw = false;
            }

            if !event::poll(Duration::from_millis(50))? {
                continue;
            }

            let event = event::read()?;
            let Event::Key(key) = event else {
                if matches!(event, Event::Resize(_, _)) {
                    needs_redraw = true;
                }
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            if self.search_mode {
                self.handle_search_input(key);
                needs_redraw = true;
                continue;
            }

            if self.command_mode {
                if self.handle_command_input(key)? {
                    break;
                }
                needs_redraw = true;
                continue;
            }

            if self.show_help {
                self.show_help = false;
                needs_redraw = true;
                continue;
            }

            if self.handle_global_keys(&key)? {
                break;
            }

            match self.branch_diff {
                Some(_) => self.handle_detail_keys(key)?,
                None if self.show_graph_view && self.focus == FocusedPane::Diff => {
                    self.handle_graph_keys(key)?
                }
                None if self.show_worktree_view && self.focus == FocusedPane::Diff => {
                    self.handle_worktree_keys(key)?
                }
                None => self.handle_branch_list_keys(key)?,
            }
            needs_redraw = true;
        }
        Ok(())
    }

    fn handle_global_keys(&mut self, key: &KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char(ch) if ch == self.config.keybindings.quit => return Ok(true),
            KeyCode::Char(ch) if ch == self.config.keybindings.help => {
                self.show_help = true;
            }
            KeyCode::Char(ch) if ch == self.config.keybindings.refresh => {
                self.refresh_repo()?;
            }
            KeyCode::Char(ch) if ch == self.config.keybindings.graph_view => {
                self.toggle_graph_view()?;
            }
            KeyCode::Char('3') => {
                self.toggle_worktree_view()?;
            }
            KeyCode::Char(ch) if ch == self.config.keybindings.command => {
                self.command_mode = true;
                self.command_input.clear();
                self.user_notice = None;
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_branch_list_keys(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('J') => self.jump_stack_group(1),
            KeyCode::Char('K') => self.jump_stack_group(-1),
            KeyCode::Char('h') | KeyCode::Left => self.fold_selected_stack_manually(),
            KeyCode::Char('l') | KeyCode::Right => self.unfold_selected_stack_manually(),
            KeyCode::Char('g') if key.modifiers.is_empty() => self.jump_to_first_branch(),
            KeyCode::Char('G') => self.jump_to_last_branch(),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(10)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-10)
            }
            KeyCode::Char(ch) if ch == self.config.keybindings.stack_view => {
                self.toggle_stack_view()
            }
            KeyCode::Char(ch) if ch == self.config.keybindings.status_view => {
                self.open_selected_status()?
            }
            KeyCode::Char('w') if self.detail_kind.is_none() => self.toggle_worktree_view()?,
            KeyCode::Esc if self.show_stack_view => self.show_stack_view = false,
            KeyCode::Esc if self.show_worktree_view => self.show_worktree_view = false,
            KeyCode::Tab if self.show_graph_view => {
                self.focus = FocusedPane::Diff;
            }
            KeyCode::Tab if self.show_worktree_view => {
                self.focus = FocusedPane::Diff;
            }
            KeyCode::Enter => self.open_selected_branch()?,
            _ => {}
        }
        Ok(())
    }

    fn handle_detail_keys(&mut self, key: KeyEvent) -> Result<()> {
        if self.dismiss_info_overlay_if_open() {
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => self.close_detail(),
            KeyCode::Tab => self.toggle_detail_tab_behavior(),
            _ => match self.focus {
                FocusedPane::BranchList => self.handle_branch_list_keys(key)?,
                FocusedPane::Diff => self.handle_diff_keys(key)?,
            },
        }
        Ok(())
    }

    fn handle_graph_keys(&mut self, key: KeyEvent) -> Result<()> {
        let visible = 10usize;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_graph_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_graph_selection(-1),
            KeyCode::Char('J') => self.jump_graph_branch_head(1),
            KeyCode::Char('K') => self.jump_graph_branch_head(-1),
            KeyCode::Char('g') if key.modifiers.is_empty() => self.graph_selected_index = 0,
            KeyCode::Char('G') => {
                self.graph_selected_index = self.graph_commits.len().saturating_sub(1)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_graph_selection(visible as isize)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_graph_selection(-(visible as isize))
            }
            KeyCode::Tab => self.focus = FocusedPane::BranchList,
            KeyCode::Esc => {
                self.show_graph_view = false;
                self.focus = FocusedPane::BranchList;
            }
            KeyCode::Enter => self.open_selected_graph_commit_branch()?,
            KeyCode::Char('e') => {}
            _ => {}
        }
        Ok(())
    }

    fn handle_worktree_keys(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_worktree_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_worktree_selection(-1),
            KeyCode::Char('g') if key.modifiers.is_empty() => self.worktree_selected_index = 0,
            KeyCode::Char('G') => {
                self.worktree_selected_index = self.worktrees.len().saturating_sub(1)
            }
            KeyCode::Tab => self.focus = FocusedPane::BranchList,
            KeyCode::Esc => {
                self.show_worktree_view = false;
                self.focus = FocusedPane::BranchList;
            }
            KeyCode::Enter => self.switch_to_selected_worktree()?,
            _ => {}
        }
        Ok(())
    }

    fn handle_diff_keys(&mut self, key: KeyEvent) -> Result<()> {
        if self.commit_list_is_open() {
            self.handle_commit_list_keys(key)?;
            return Ok(());
        }

        let visible = 15usize;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.scroll_diff(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_diff(-1),
            KeyCode::Char('J') => self.jump_file(1),
            KeyCode::Char('K') => self.jump_file(-1),
            KeyCode::Char('g') => {
                if key.modifiers.is_empty() {
                    self.scroll_diff_to_top();
                }
            }
            KeyCode::Char('G') => self.scroll_diff_to_bottom(),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_diff(visible as isize / 2)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_diff(-(visible as isize / 2))
            }
            KeyCode::Char('/') => {
                self.search_mode = true;
                self.search_input.clear();
            }
            KeyCode::Char('v') => self.toggle_diff_view(),
            KeyCode::Char('w') => self.toggle_whitespace_mode()?,
            KeyCode::Char('n') => self.advance_match(1),
            KeyCode::Char('N') => self.advance_match(-1),
            KeyCode::Char('i')
                if self.detail_kind == Some(DetailKind::BranchDiff)
                    && self.branch_detail.is_some() =>
            {
                self.show_branch_info_overlay();
            }
            KeyCode::Backspace
                if matches!(
                    self.branch_detail.as_ref().map(|detail| &detail.diff_mode),
                    Some(BranchDetailDiffMode::Commit { .. })
                ) =>
            {
                self.restore_branch_level_diff()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search_mode = false;
                self.search_input.clear();
            }
            KeyCode::Enter => {
                self.search_mode = false;
                self.refresh_search_matches();
                self.advance_match(1);
            }
            KeyCode::Backspace => {
                self.search_input.pop();
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.search_input.push(ch);
                }
            }
            _ => {}
        }
    }

    fn handle_command_input(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.command_mode = false;
                self.command_input.clear();
            }
            KeyCode::Enter => {
                self.command_mode = false;
                let command = mem::take(&mut self.command_input);
                return self.execute_command(command.trim());
            }
            KeyCode::Backspace => {
                self.command_input.pop();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.command_input.push(ch);
            }
            _ => {}
        }
        Ok(false)
    }

    fn execute_command(&mut self, command: &str) -> Result<bool> {
        self.user_notice = None;
        if command.is_empty() {
            return Ok(false);
        }
        if command == "q" {
            return Ok(true);
        }
        if let Some(branch) = command.strip_prefix("branch ") {
            let branch = branch.trim();
            if self.jump_to_branch(branch) {
                return Ok(false);
            }
            self.user_notice = Some(format!("No branch matched `{branch}`."));
            return Ok(false);
        }
        if let Some(term) = command.strip_prefix("search ") {
            let term = term.trim();
            if term.is_empty() {
                self.user_notice = Some("Search term cannot be empty.".to_string());
            } else if self.search_branch_list(term) {
                self.user_notice = Some(format!("Matched branch search `{term}`."));
            } else {
                self.user_notice = Some(format!("No branches contained `{term}`."));
            }
            return Ok(false);
        }
        self.user_notice = Some(format!("Unknown command `:{command}`."));
        Ok(false)
    }

    fn move_selection(&mut self, delta: isize) {
        if delta == 0 {
            return;
        }

        let ordered_branches = ordered_branch_names(&self.repo, &self.stack_info);
        if ordered_branches.is_empty() {
            self.selected_index = self
                .display_entries
                .iter()
                .position(|entry| !entry.is_header())
                .unwrap_or(0);
            return;
        }

        let Some(current_branch) = self.selection_anchor_branch(delta).map(ToOwned::to_owned) else {
            return;
        };
        let current_pos = ordered_branches
            .iter()
            .position(|branch| branch == &current_branch)
            .unwrap_or(0);
        let step = delta.signum();
        let mut next_pos = current_pos as isize;

        loop {
            let candidate = (next_pos + step).clamp(0, (ordered_branches.len() - 1) as isize);
            if candidate == next_pos {
                return;
            }
            next_pos = candidate;

            let next_branch = &ordered_branches[next_pos as usize];
            if self.branch_is_hidden_by_manual_fold(next_branch) {
                continue;
            }

            self.apply_implicit_stack_navigation(&current_branch, next_branch);
            return;
        }
    }

    fn selection_anchor_branch(&self, delta: isize) -> Option<&str> {
        if let Some(branch_name) = self.selected_branch_name() {
            return Some(branch_name);
        }

        let BranchEntry::Header { label, expanded } = self.display_entries.get(self.selected_index)?
        else {
            return None;
        };
        if expanded.is_none() {
            return None;
        }

        let stack = self
            .stack_info
            .stacks
            .iter()
            .find(|stack| stack.name == *label)?;
        if delta.is_negative() {
            stack.branches.first().map(String::as_str)
        } else {
            stack.branches.last().map(String::as_str)
        }
    }

    fn branch_is_hidden_by_manual_fold(&self, branch_name: &str) -> bool {
        stack_name_for_branch(&self.stack_info, branch_name)
            .and_then(|stack_name| self.manual_stack_states.get(stack_name))
            .is_some_and(|expanded| !expanded)
    }

    fn apply_implicit_stack_navigation(&mut self, current_branch: &str, next_branch: &str) {
        let current_stack = stack_name_for_branch(&self.stack_info, current_branch);
        let next_stack = stack_name_for_branch(&self.stack_info, next_branch);
        let mut changed = false;

        if current_stack != next_stack {
            if let Some(auto_stack) = self.auto_expanded_stack.clone() {
                if Some(auto_stack.as_str()) != next_stack
                    && !self.manual_stack_states.contains_key(&auto_stack)
                {
                    changed |= self.expanded_stacks.remove(&auto_stack);
                    self.auto_expanded_stack = None;
                }
            }

            if let Some(stack_name) = next_stack {
                if !self.expanded_stacks.contains(stack_name)
                    && !self.manual_stack_states.contains_key(stack_name)
                {
                    self.expanded_stacks.insert(stack_name.to_string());
                    self.auto_expanded_stack = Some(stack_name.to_string());
                    changed = true;
                }
            }
        }

        if changed {
            self.rebuild_display_entries_preserve_selection(Some(next_branch));
        } else {
            self.select_branch(next_branch);
        }
    }

    fn selected_stack_name(&self) -> Option<&str> {
        if let Some(branch_name) = self.selected_branch_name() {
            return stack_name_for_branch(&self.stack_info, branch_name);
        }

        let BranchEntry::Header { label, expanded } = self.display_entries.get(self.selected_index)?
        else {
            return None;
        };
        expanded.is_some().then_some(label.as_str())
    }

    fn fold_selected_stack_manually(&mut self) {
        let Some(stack_name) = self.selected_stack_name().map(ToOwned::to_owned) else {
            return;
        };

        self.manual_stack_states.insert(stack_name.clone(), false);
        self.expanded_stacks.remove(&stack_name);
        if self.auto_expanded_stack.as_deref() == Some(stack_name.as_str()) {
            self.auto_expanded_stack = None;
        }

        self.rebuild_display_entries_preserve_selection(None);
        if let Some(index) = self.display_entries.iter().position(
            |entry| matches!(entry, BranchEntry::Header { label, .. } if label == &stack_name),
        ) {
            self.selected_index = index;
        }
    }

    fn unfold_selected_stack_manually(&mut self) {
        let Some(stack_name) = self.selected_stack_name().map(ToOwned::to_owned) else {
            return;
        };

        self.manual_stack_states.insert(stack_name.clone(), true);
        self.expanded_stacks.insert(stack_name.clone());
        if self.auto_expanded_stack.as_deref() == Some(stack_name.as_str()) {
            self.auto_expanded_stack = None;
        }

        let selected_branch = self.selected_branch_name().map(ToOwned::to_owned);
        self.rebuild_display_entries_preserve_selection(selected_branch.as_deref());
        if selected_branch.is_none() {
            if let Some(index) = self.display_entries.iter().position(|entry| {
                !entry.is_header()
                    && stack_name_for_branch(&self.stack_info, entry.branch_name())
                        == Some(stack_name.as_str())
            }) {
                self.selected_index = index;
            }
        }
    }

    fn jump_stack_group(&mut self, direction: isize) {
        // Find header positions
        let headers: Vec<usize> = self
            .display_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_header())
            .map(|(i, _)| i)
            .collect();

        if headers.is_empty() {
            return;
        }

        // Find the first selectable entry after each header
        let group_starts: Vec<usize> = headers
            .iter()
            .filter_map(|&h| {
                self.display_entries
                    .iter()
                    .enumerate()
                    .skip(h + 1)
                    .find(|(_, e)| !e.is_header())
                    .map(|(i, _)| i)
            })
            .collect();

        if group_starts.is_empty() {
            return;
        }

        let current_group = group_starts
            .iter()
            .rposition(|&s| s <= self.selected_index)
            .unwrap_or(0);
        let next_group =
            (current_group as isize + direction).clamp(0, (group_starts.len() - 1) as isize);
        self.selected_index = group_starts[next_group as usize];
    }

    fn jump_to_first_branch(&mut self) {
        if let Some(pos) = self.display_entries.iter().position(|e| !e.is_header()) {
            self.selected_index = pos;
        }
    }

    fn jump_to_last_branch(&mut self) {
        if let Some(pos) = self.display_entries.iter().rposition(|e| !e.is_header()) {
            self.selected_index = pos;
        }
    }

    fn open_selected_branch(&mut self) -> Result<()> {
        let _timer = perf::ScopeTimer::new("open_selected_branch");
        let entry = match self.display_entries.get(self.selected_index) {
            Some(e) if !e.is_header() => e.clone(),
            _ => return Ok(()),
        };

        let Some(branch) = branch_for_diff(&self.repo, &self.stack_info, entry.branch_name())
        else {
            return Ok(());
        };

        let branch_name = branch.name.clone();
        let preloaded = self
            .preloaded_diffs
            .get(&branch_name)
            .cloned()
            .or_else(|| self.wait_for_preloaded_diff(&branch_name));
        if let Some(preloaded) = preloaded {
            self.highlighted_diff = Some(preloaded.highlighted_diff);
            self.branch_diff = Some(preloaded.diff);
        } else {
            self.start_single_diff_preload(branch.clone());
            if let Some(preloaded) = self.wait_for_preloaded_diff(&branch_name) {
                self.highlighted_diff = Some(preloaded.highlighted_diff);
                self.branch_diff = Some(preloaded.diff);
            } else {
                let preloaded =
                    preload_branch_diff(&self.repo.root, branch.clone(), self.diff_options())?;
                self.preloaded_diffs
                    .insert(branch_name.clone(), preloaded.clone());
                self.highlighted_diff = Some(preloaded.highlighted_diff);
                self.branch_diff = Some(preloaded.diff);
            }
        }
        let commits = load_branch_commits(&self.repo.root, &branch)?;
        self.detail_kind = Some(DetailKind::BranchDiff);
        self.branch_detail = Some(BranchDetailState {
            branch_name,
            commits,
            commit_list_open: false,
            commit_list_selected: 0,
            diff_mode: BranchDetailDiffMode::Branch,
            info_overlay: None,
        });
        self.show_stack_view = false;
        self.diff_scroll = 0;
        self.focus = FocusedPane::Diff;
        self.search_input.clear();
        self.search_matches.clear();
        self.search_cursor = 0;
        Ok(())
    }

    fn open_selected_status(&mut self) -> Result<()> {
        let Some(selected_branch) = self.selected_branch_name() else {
            return Ok(());
        };
        let Some(branch) = self
            .repo
            .branches
            .iter()
            .find(|branch| branch.name == selected_branch)
        else {
            return Ok(());
        };
        if !branch.is_head {
            return Ok(());
        }

        let diff = load_working_tree_status(&self.repo.root, &branch.name, self.diff_options())?;
        let highlighted_diff = SyntaxHighlighter::new().highlight_diff(&diff)?;
        self.detail_kind = Some(DetailKind::Status);
        self.branch_detail = None;
        self.branch_diff = Some(diff);
        self.highlighted_diff = Some(highlighted_diff);
        self.show_stack_view = false;
        self.diff_scroll = 0;
        self.focus = FocusedPane::Diff;
        self.search_input.clear();
        self.search_matches.clear();
        self.search_cursor = 0;
        Ok(())
    }

    fn close_detail(&mut self) {
        self.detail_kind = None;
        self.branch_detail = None;
        self.branch_diff = None;
        self.highlighted_diff = None;
        self.diff_scroll = 0;
        self.focus = FocusedPane::BranchList;
        self.search_input.clear();
        self.search_matches.clear();
        self.search_cursor = 0;
    }

    fn refresh_repo(&mut self) -> Result<()> {
        let _timer = perf::ScopeTimer::new("App::refresh_repo");
        self.repo = refresh_repo(&self.repo.root)?;
        self.worktrees = load_worktrees(&self.repo.root).unwrap_or_default();
        self.stack_info = detect_stacks(&self.repo.root, &self.repo, false);
        self.reset_diff_preload_state();
        self.rebuild_display_entries_preserve_selection(None);
        if self.show_stack_view && self.current_stack_view().is_none() {
            self.show_stack_view = false;
        }
        if self.show_graph_view {
            self.graph_commits = load_commit_graph(&self.repo.root, &self.repo)?;
            self.graph_selected_index = self
                .graph_selected_index
                .min(self.graph_commits.len().saturating_sub(1));
        }
        if self.show_worktree_view {
            self.worktree_selected_index = self
                .worktrees
                .iter()
                .position(|worktree| worktree.is_active)
                .unwrap_or(0);
        }
        self.reload_stack_decorations_async();
        self.reload_commit_counts_async();
        self.start_diff_preload_async();

        // Clamp selected_index to a valid selectable entry
        let selectable: Vec<usize> = self
            .display_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_header())
            .map(|(i, _)| i)
            .collect();
        if selectable.is_empty() {
            self.selected_index = 0;
        } else if !selectable.contains(&self.selected_index) {
            self.selected_index = *selectable.last().unwrap();
        }

        if let Some(detail_kind) = self.detail_kind {
            match detail_kind {
                DetailKind::BranchDiff => {
                    let Some(branch_name) = self
                        .branch_detail
                        .as_ref()
                        .map(|detail| detail.branch_name.clone())
                    else {
                        self.close_detail();
                        return Ok(());
                    };

                    if let Some(branch) =
                        branch_for_diff(&self.repo, &self.stack_info, &branch_name)
                    {
                        let preloaded = preload_branch_diff(
                            &self.repo.root,
                            branch.clone(),
                            self.diff_options(),
                        )?;
                        self.preloaded_diffs
                            .insert(branch_name.clone(), preloaded.clone());
                        self.highlighted_diff = Some(preloaded.highlighted_diff);
                        self.branch_diff = Some(preloaded.diff);
                        if let Some(detail) = &mut self.branch_detail {
                            detail.commits = load_branch_commits(&self.repo.root, &branch)?;
                            detail.commit_list_selected = detail
                                .commit_list_selected
                                .min(detail.commits.len().saturating_sub(1));
                            detail.commit_list_open = false;
                            detail.diff_mode = BranchDetailDiffMode::Branch;
                            detail.info_overlay = None;
                        }
                        self.refresh_search_matches();
                    } else {
                        self.close_detail();
                    }
                }
                DetailKind::Status => {
                    if let Some(head_branch) =
                        self.repo.branches.iter().find(|branch| branch.is_head)
                    {
                        let diff = load_working_tree_status(
                            &self.repo.root,
                            &head_branch.name,
                            self.diff_options(),
                        )?;
                        let highlighted_diff = SyntaxHighlighter::new().highlight_diff(&diff)?;
                        self.branch_diff = Some(diff);
                        self.highlighted_diff = Some(highlighted_diff);
                        self.refresh_search_matches();
                    } else {
                        self.close_detail();
                    }
                }
            }
        }

        Ok(())
    }

    fn apply_pending_watch_events(&mut self) -> Result<bool> {
        let mut refresh_requested = false;
        while let Ok(message) = self.watch_event_rx.try_recv() {
            if message == WatchMessage::RefreshRequested {
                refresh_requested = true;
            }
        }

        if !refresh_requested {
            return Ok(false);
        }

        self.refresh_repo()?;
        Ok(true)
    }

    fn reload_stack_decorations_async(&mut self) {
        self.stack_request_id += 1;
        let request_id = self.stack_request_id;
        let root = self.repo.root.clone();
        let stack_info = self.stack_info.clone();
        let tx = self.stack_result_tx.clone();
        thread::spawn(move || {
            let _timer = perf::ScopeTimer::new(format!(
                "reload_stack_decorations_async request={request_id}"
            ));
            let stack_info = enrich_stacks(&root, &stack_info);
            let _ = tx.send(StackLoadResult {
                request_id,
                stack_info,
            });
        });
    }

    fn reload_commit_counts_async(&mut self) {
        self.commit_count_request_id += 1;
        let request_id = self.commit_count_request_id;
        let root = self.repo.root.clone();
        let repo = self.repo.clone();
        let tx = self.commit_count_result_tx.clone();
        thread::spawn(move || {
            let _timer =
                perf::ScopeTimer::new(format!("reload_commit_counts_async request={request_id}"));
            let commit_counts = load_commit_counts(&root, &repo);
            let _ = tx.send(CommitCountLoadResult {
                request_id,
                commit_counts,
            });
        });
    }

    fn start_diff_preload_async(&mut self) {
        if self.diff_preload_started {
            return;
        }

        self.diff_preload_started = true;
        self.diff_preload_request_id += 1;
        let request_id = self.diff_preload_request_id;
        for branch in diff_preload_targets(&self.repo, &self.stack_info, &self.display_entries) {
            self.start_single_diff_preload_with_request(branch, request_id);
        }
    }

    fn start_single_diff_preload(&mut self, branch: BranchInfo) {
        if !self.diff_preload_started {
            self.diff_preload_started = true;
            self.diff_preload_request_id += 1;
        }
        let request_id = self.diff_preload_request_id;
        self.start_single_diff_preload_with_request(branch, request_id);
    }

    fn start_single_diff_preload_with_request(&mut self, branch: BranchInfo, request_id: usize) {
        if self.preloaded_diffs.contains_key(&branch.name)
            || self.pending_diff_preloads.contains(&branch.name)
        {
            return;
        }

        let root = self.repo.root.clone();
        let branch_name = branch.name.clone();
        let options = self.diff_options();
        let tx = self.diff_preload_result_tx.clone();
        self.pending_diff_preloads.insert(branch_name.clone());
        thread::spawn(move || {
            let _timer = perf::ScopeTimer::new(format!("diff_preload branch={}", branch_name));
            let preloaded =
                preload_branch_diff(&root, branch, options).map_err(|err| format!("{err:#}"));
            let _ = tx.send(DiffPreloadResult {
                request_id,
                branch_name,
                preloaded,
            });
        });
    }

    fn apply_pending_stack_updates(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.stack_result_rx.try_recv() {
            if result.request_id != self.stack_request_id {
                continue;
            }

            let selected_branch = self.selected_branch_name().map(ToOwned::to_owned);
            self.stack_info = result.stack_info;
            self.sync_expanded_stacks(selected_branch.as_deref());
            self.rebuild_display_entries_preserve_selection(selected_branch.as_deref());
            changed = true;
        }
        changed
    }

    fn apply_pending_diff_preloads(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.diff_preload_result_rx.try_recv() {
            if result.request_id != self.diff_preload_request_id {
                continue;
            }

            self.pending_diff_preloads.remove(&result.branch_name);
            match result.preloaded {
                Ok(preloaded) => {
                    self.preloaded_diffs.insert(result.branch_name, preloaded);
                }
                Err(message) => {
                    self.diff_preload_failures
                        .insert(result.branch_name, message);
                }
            }
            changed = true;
        }
        changed
    }

    fn wait_for_preloaded_diff(&mut self, branch_name: &str) -> Option<PreloadedBranchDiff> {
        if let Some(preloaded) = self.preloaded_diffs.get(branch_name).cloned() {
            return Some(preloaded);
        }
        if !self.pending_diff_preloads.contains(branch_name) {
            return None;
        }

        while self.pending_diff_preloads.contains(branch_name) {
            let Ok(result) = self.diff_preload_result_rx.recv() else {
                break;
            };
            if result.request_id != self.diff_preload_request_id {
                continue;
            }

            self.pending_diff_preloads.remove(&result.branch_name);
            match result.preloaded {
                Ok(preloaded) => {
                    self.preloaded_diffs
                        .insert(result.branch_name.clone(), preloaded.clone());
                }
                Err(message) => {
                    self.diff_preload_failures
                        .insert(result.branch_name.clone(), message);
                }
            }
        }

        self.preloaded_diffs.get(branch_name).cloned()
    }

    fn reset_diff_preload_state(&mut self) {
        self.diff_preload_started = false;
        self.diff_preload_request_id += 1;
        self.preloaded_diffs.clear();
        self.diff_preload_failures.clear();
        self.pending_diff_preloads.clear();
    }

    fn apply_pending_commit_count_updates(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.commit_count_result_rx.try_recv() {
            if result.request_id != self.commit_count_request_id {
                continue;
            }

            let selected_branch = self.selected_branch_name().map(ToOwned::to_owned);
            for (name, commit_count) in result.commit_counts {
                if let Some(branch) = self
                    .repo
                    .branches
                    .iter_mut()
                    .find(|branch| branch.name == name)
                {
                    branch.commit_count = commit_count;
                    changed = true;
                }
            }

            if changed {
                self.rebuild_display_entries_preserve_selection(selected_branch.as_deref());
            }
        }
        changed
    }

    fn toggle_stack_view(&mut self) {
        if self.show_stack_view {
            self.show_stack_view = false;
            return;
        }
        self.show_stack_view = self.stack_view_for_selected_branch().is_some();
    }

    fn current_stack_view(&self) -> Option<StackView> {
        if !self.show_stack_view {
            return None;
        }

        self.stack_view_for_selected_branch()
    }

    fn current_graph_view(&self) -> Option<GraphView<'_>> {
        self.show_graph_view.then_some(GraphView {
            commits: &self.graph_commits,
            selected_index: self.graph_selected_index,
        })
    }

    fn current_worktree_view(&self) -> Option<WorktreeView<'_>> {
        self.show_worktree_view.then_some(WorktreeView {
            worktrees: &self.worktrees,
            selected_index: self.worktree_selected_index,
        })
    }

    fn stack_view_for_selected_branch(&self) -> Option<StackView> {
        let selected_branch = self.selected_branch_name()?;
        build_stack_view(&self.repo, &self.stack_info, selected_branch)
    }

    fn toggle_graph_view(&mut self) -> Result<()> {
        if self.show_graph_view {
            self.show_graph_view = false;
            self.focus = FocusedPane::BranchList;
            return Ok(());
        }

        self.graph_commits = load_commit_graph(&self.repo.root, &self.repo)?;
        self.graph_selected_index = 0;
        self.show_graph_view = true;
        self.show_stack_view = false;
        self.show_worktree_view = false;
        self.focus = FocusedPane::Diff;
        Ok(())
    }

    fn toggle_worktree_view(&mut self) -> Result<()> {
        if self.show_worktree_view {
            self.show_worktree_view = false;
            self.focus = FocusedPane::BranchList;
            return Ok(());
        }
        self.worktrees = load_worktrees(&self.repo.root)?;
        self.worktree_selected_index = self
            .worktrees
            .iter()
            .position(|worktree| worktree.is_active)
            .unwrap_or(0);
        self.show_worktree_view = true;
        self.show_stack_view = false;
        self.show_graph_view = false;
        self.focus = FocusedPane::Diff;
        Ok(())
    }

    fn move_graph_selection(&mut self, delta: isize) {
        if self.graph_commits.is_empty() {
            self.graph_selected_index = 0;
            return;
        }
        let max = self.graph_commits.len().saturating_sub(1) as isize;
        self.graph_selected_index =
            (self.graph_selected_index as isize + delta).clamp(0, max) as usize;
    }

    fn jump_graph_branch_head(&mut self, direction: isize) {
        let branch_heads: Vec<_> = self
            .graph_commits
            .iter()
            .enumerate()
            .filter(|(_, commit)| !commit.branch_labels.is_empty())
            .map(|(index, _)| index)
            .collect();
        if branch_heads.is_empty() {
            return;
        }
        let current_group = branch_heads
            .iter()
            .rposition(|&index| index <= self.graph_selected_index)
            .unwrap_or(0);
        let next = (current_group as isize + direction).clamp(0, branch_heads.len() as isize - 1);
        self.graph_selected_index = branch_heads[next as usize];
    }

    fn open_selected_graph_commit_branch(&mut self) -> Result<()> {
        let Some(commit) = self.graph_commits.get(self.graph_selected_index) else {
            return Ok(());
        };
        let Some(branch_name) = commit.primary_branch.as_deref() else {
            return Ok(());
        };
        if let Some(index) = self
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == branch_name)
        {
            self.selected_index = index;
        }
        self.open_selected_branch()
    }

    fn move_worktree_selection(&mut self, delta: isize) {
        if self.worktrees.is_empty() {
            self.worktree_selected_index = 0;
            return;
        }
        let max = self.worktrees.len().saturating_sub(1) as isize;
        self.worktree_selected_index =
            (self.worktree_selected_index as isize + delta).clamp(0, max) as usize;
    }

    fn switch_to_selected_worktree(&mut self) -> Result<()> {
        let Some(worktree) = self.worktrees.get(self.worktree_selected_index).cloned() else {
            return Ok(());
        };
        if worktree.is_bare {
            self.user_notice = Some("Cannot switch into a bare worktree entry.".to_string());
            return Ok(());
        }

        self.repo = refresh_repo(&worktree.path)?;
        self.worktrees = load_worktrees(&worktree.path)?;
        self.stack_info = detect_stacks(&self.repo.root, &self.repo, false);
        let (repo_watcher, watch_notice) =
            match start_repo_watcher(&self.repo.root, self._watch_event_tx.clone()) {
                Ok(watcher) => (Some(watcher), None),
                Err(error) => (
                    None,
                    Some(format!(
                        "Filesystem watching unavailable; use R to refresh ({error})"
                    )),
                ),
            };
        self._repo_watcher = repo_watcher;
        self.watch_notice = watch_notice;
        self.graph_commits.clear();
        self.show_worktree_view = true;
        self.worktree_selected_index = self
            .worktrees
            .iter()
            .position(|candidate| candidate.path == worktree.path)
            .unwrap_or(0);
        self.reset_diff_preload_state();
        self.rebuild_display_entries_preserve_selection(None);
        self.reload_stack_decorations_async();
        self.reload_commit_counts_async();
        self.start_diff_preload_async();
        if let Some(head_branch_name) = self
            .repo
            .branches
            .iter()
            .find(|branch| branch.is_head)
            .map(|branch| branch.name.clone())
        {
            let _ = self.jump_to_branch(&head_branch_name);
        }
        self.user_notice = Some(format!("Switched to worktree {}", worktree.path.display()));
        Ok(())
    }

    fn rebuild_display_entries_preserve_selection(&mut self, branch_name: Option<&str>) {
        let selected_branch = branch_name
            .map(ToOwned::to_owned)
            .or_else(|| self.selected_branch_name().map(ToOwned::to_owned));
        self.display_entries = build_display_entries(
            &self.repo,
            &self.stack_info,
            &self.worktrees,
            &self.expanded_stacks,
        );

        if let Some(branch_name) = selected_branch {
            if let Some(index) = self
                .display_entries
                .iter()
                .position(|entry| !entry.is_header() && entry.branch_name() == branch_name)
            {
                self.selected_index = index;
                return;
            }
        }

        self.selected_index = self
            .display_entries
            .iter()
            .position(|entry| !entry.is_header())
            .unwrap_or(0);
    }

    fn selected_branch_name(&self) -> Option<&str> {
        self.display_entries
            .get(self.selected_index)
            .and_then(|entry| {
                if entry.is_header() {
                    None
                } else {
                    Some(entry.branch_name())
                }
            })
    }

    fn jump_to_branch(&mut self, branch_name: &str) -> bool {
        self.expand_stack_for_branch(branch_name);
        let Some(index) = self
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == branch_name)
        else {
            return false;
        };
        self.selected_index = index;
        self.show_graph_view = false;
        self.show_stack_view = false;
        self.focus = FocusedPane::BranchList;
        true
    }

    fn search_branch_list(&mut self, term: &str) -> bool {
        let term = term.to_lowercase();
        let Some(index) = self.display_entries.iter().position(|entry| {
            !entry.is_header() && entry.branch_name().to_lowercase().contains(&term)
        }) else {
            return false;
        };
        self.selected_index = index;
        self.show_graph_view = false;
        self.focus = FocusedPane::BranchList;
        true
    }

    fn sync_expanded_stacks(&mut self, selected_branch: Option<&str>) {
        let valid_stack_names: HashSet<_> = self
            .stack_info
            .stacks
            .iter()
            .map(|stack| stack.name.clone())
            .collect();
        self.expanded_stacks.retain(|stack| valid_stack_names.contains(stack));
        self.manual_stack_states
            .retain(|stack, _| valid_stack_names.contains(stack));
        if self
            .auto_expanded_stack
            .as_ref()
            .is_some_and(|stack| !valid_stack_names.contains(stack))
        {
            self.auto_expanded_stack = None;
        }
        for stack in &self.stack_info.stacks {
            if !self.expanded_stacks.contains(&stack.name)
                && !matches!(self.manual_stack_states.get(&stack.name), Some(false))
                && stack_contains_head(&self.repo, stack)
            {
                self.expanded_stacks.insert(stack.name.clone());
            }
        }
        if let Some(branch_name) = selected_branch {
            self.expand_stack_for_branch(branch_name);
        }
    }

    fn expand_stack_for_branch(&mut self, branch_name: &str) {
        if let Some(stack_name) = stack_name_for_branch(&self.stack_info, branch_name) {
            if matches!(self.manual_stack_states.get(stack_name), Some(false)) {
                return;
            }
            self.expanded_stacks.insert(stack_name.to_string());
        }
    }

    fn select_branch(&mut self, branch_name: &str) {
        if let Some(index) = self
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == branch_name)
        {
            self.selected_index = index;
            return;
        }

        self.rebuild_display_entries_preserve_selection(Some(branch_name));
    }

    fn scroll_diff(&mut self, delta: isize) {
        let max_scroll = self
            .branch_diff
            .as_ref()
            .map(|diff| diff.lines.len().saturating_sub(1))
            .unwrap_or(0) as isize;
        let next = self.diff_scroll as isize + delta;
        self.diff_scroll = next.clamp(0, max_scroll) as usize;
    }

    fn scroll_diff_to_top(&mut self) {
        self.diff_scroll = 0;
    }

    fn scroll_diff_to_bottom(&mut self) {
        if let Some(diff) = &self.branch_diff {
            self.diff_scroll = diff.lines.len().saturating_sub(1);
        }
    }

    fn jump_file(&mut self, direction: isize) {
        let Some(diff) = &self.branch_diff else {
            return;
        };
        if diff.file_positions.is_empty() {
            return;
        }

        let current = self.diff_scroll;
        let next = if direction > 0 {
            diff.file_positions
                .iter()
                .copied()
                .find(|position| *position > current)
                .or_else(|| diff.file_positions.last().copied())
        } else {
            diff.file_positions
                .iter()
                .rev()
                .copied()
                .find(|position| *position < current)
                .or_else(|| diff.file_positions.first().copied())
        };

        if let Some(next) = next {
            self.diff_scroll = next;
        }
    }

    fn refresh_search_matches(&mut self) {
        let Some(diff) = &self.branch_diff else {
            self.search_matches.clear();
            return;
        };
        if self.search_input.is_empty() {
            self.search_matches.clear();
            return;
        }
        let needle = self.search_input.to_lowercase();
        self.search_matches = diff
            .lines
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| line.text.to_lowercase().contains(&needle).then_some(idx))
            .collect();
        self.search_cursor = 0;
    }

    fn advance_match(&mut self, direction: isize) {
        if self.search_matches.is_empty() {
            return;
        }
        let len = self.search_matches.len() as isize;
        let next = (self.search_cursor as isize + direction).rem_euclid(len);
        self.search_cursor = next as usize;
        if let Some(position) = self.search_matches.get(self.search_cursor).copied() {
            self.diff_scroll = position;
        }
    }

    fn restore_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        disable_raw_mode().context("failed to disable raw mode")?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)
            .context("failed to leave alternate screen")?;
        terminal.show_cursor().ok();
        mem::take(&mut self.search_input);
        Ok(())
    }

    fn toggle_detail_tab_behavior(&mut self) {
        if self.detail_kind == Some(DetailKind::BranchDiff) && self.branch_detail.is_some() {
            if let Some(detail) = &mut self.branch_detail {
                detail.info_overlay = None;
                detail.commit_list_open = !detail.commit_list_open;
            }
            return;
        }

        self.focus = match self.focus {
            FocusedPane::BranchList => FocusedPane::Diff,
            FocusedPane::Diff => FocusedPane::BranchList,
        };
    }

    fn commit_list_is_open(&self) -> bool {
        self.branch_detail
            .as_ref()
            .map(|detail| detail.commit_list_open)
            .unwrap_or(false)
    }

    fn commit_list_overlay_items(&self) -> Option<Vec<String>> {
        let detail = self.branch_detail.as_ref()?;
        if !detail.commit_list_open {
            return None;
        }

        if detail.commits.is_empty() {
            return Some(vec!["No commits above the current base branch.".to_string()]);
        }

        Some(
            detail
                .commits
                .iter()
                .map(|commit| {
                    format!(
                        "{:<8}  {:<10}  {}",
                        commit.short_oid, commit.committed_at, commit.subject
                    )
                })
                .collect(),
        )
    }

    fn commit_list_overlay_selected(&self) -> Option<usize> {
        self.branch_detail.as_ref().and_then(|detail| {
            detail
                .commit_list_open
                .then_some(detail.commit_list_selected)
        })
    }

    fn handle_commit_list_keys(&mut self, key: KeyEvent) -> Result<()> {
        let Some(detail) = &mut self.branch_detail else {
            return Ok(());
        };

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if !detail.commits.is_empty() {
                    detail.commit_list_selected =
                        (detail.commit_list_selected + 1).min(detail.commits.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                detail.commit_list_selected = detail.commit_list_selected.saturating_sub(1);
            }
            KeyCode::Char('g') if key.modifiers.is_empty() => {
                detail.commit_list_selected = 0;
            }
            KeyCode::Char('G') => {
                if !detail.commits.is_empty() {
                    detail.commit_list_selected = detail.commits.len() - 1;
                }
            }
            KeyCode::Enter => self.open_selected_commit_diff()?,
            KeyCode::Tab => detail.commit_list_open = false,
            _ => {}
        }

        Ok(())
    }

    fn open_selected_commit_diff(&mut self) -> Result<()> {
        let options = self.diff_options();
        let Some(detail) = &mut self.branch_detail else {
            return Ok(());
        };
        let Some(commit) = detail.commits.get(detail.commit_list_selected).cloned() else {
            return Ok(());
        };

        let diff = load_commit_diff(&self.repo.root, &detail.branch_name, &commit, options)?;
        let highlighted_diff = SyntaxHighlighter::new().highlight_diff(&diff)?;
        self.branch_diff = Some(diff);
        self.highlighted_diff = Some(highlighted_diff);
        detail.diff_mode = BranchDetailDiffMode::Commit {
            selected_index: detail.commit_list_selected,
        };
        detail.commit_list_open = false;
        self.diff_scroll = 0;
        self.search_input.clear();
        self.search_matches.clear();
        self.search_cursor = 0;
        Ok(())
    }

    fn restore_branch_level_diff(&mut self) -> Result<()> {
        let Some(branch_name) = self
            .branch_detail
            .as_ref()
            .map(|detail| detail.branch_name.clone())
        else {
            return Ok(());
        };
        let Some(preloaded) = self
            .preloaded_diffs
            .get(&branch_name)
            .cloned()
            .or_else(|| self.wait_for_preloaded_diff(&branch_name))
        else {
            return Ok(());
        };
        self.branch_diff = Some(preloaded.diff);
        self.highlighted_diff = Some(preloaded.highlighted_diff);
        if let Some(detail) = &mut self.branch_detail {
            detail.diff_mode = BranchDetailDiffMode::Branch;
        }
        self.diff_scroll = 0;
        self.search_input.clear();
        self.search_matches.clear();
        self.search_cursor = 0;
        Ok(())
    }

    fn show_branch_info_overlay(&mut self) {
        let Some(detail) = &mut self.branch_detail else {
            return;
        };
        let Some(branch) = self
            .repo
            .branches
            .iter()
            .find(|branch| branch.name == detail.branch_name)
        else {
            return;
        };

        let stack_position = self
            .stack_info
            .stacks
            .iter()
            .find_map(|stack| {
                stack
                    .branches
                    .iter()
                    .position(|name| name == &detail.branch_name)
                    .map(|index| {
                        format!(
                            "{} of {} in {}",
                            index + 1,
                            stack.branches.len(),
                            stack.name
                        )
                    })
            })
            .unwrap_or_else(|| "standalone".to_string());
        let remote_status = match (&branch.upstream, branch.ahead, branch.behind) {
            (Some(upstream), 0, 0) => format!("{upstream} (in sync)"),
            (Some(upstream), ahead, behind) => {
                format!("{upstream} (ahead {ahead}, behind {behind})")
            }
            (None, _, _) => "no upstream".to_string(),
        };

        detail.info_overlay = Some(vec![
            format!("Branch: {}", detail.branch_name),
            format!(
                "Base branch: {}",
                branch.base_ref.as_deref().unwrap_or("none")
            ),
            format!("Remote status: {remote_status}"),
            format!("Worktree: {}", self.repo.root.display()),
            format!("Stack position: {stack_position}"),
        ]);
        detail.commit_list_open = false;
    }

    fn dismiss_info_overlay_if_open(&mut self) -> bool {
        if let Some(detail) = &mut self.branch_detail {
            if detail.info_overlay.is_some() {
                detail.info_overlay = None;
                return true;
            }
        }
        false
    }

    fn current_notice(&self) -> Option<&str> {
        self.user_notice
            .as_deref()
            .or(self.watch_notice.as_deref())
            .or_else(|| stack_notice(&self.stack_info))
    }

    fn diff_options(&self) -> DiffOptions {
        DiffOptions {
            ignore_whitespace: self.ignore_whitespace,
        }
    }

    fn toggle_diff_view(&mut self) {
        self.diff_view = match self.diff_view {
            DiffViewMode::Unified => DiffViewMode::SideBySide,
            DiffViewMode::SideBySide => DiffViewMode::Unified,
        };
    }

    fn toggle_whitespace_mode(&mut self) -> Result<()> {
        if self.detail_kind.is_none() {
            return Ok(());
        }

        self.ignore_whitespace = !self.ignore_whitespace;
        self.reset_diff_preload_state();
        match self.detail_kind {
            Some(DetailKind::BranchDiff) => {
                if matches!(
                    self.branch_detail.as_ref().map(|detail| &detail.diff_mode),
                    Some(BranchDetailDiffMode::Commit { .. })
                ) {
                    self.open_selected_commit_diff()?;
                } else {
                    self.open_selected_branch()?;
                }
            }
            Some(DetailKind::Status) => self.open_selected_status()?,
            None => {}
        }
        Ok(())
    }
}

fn load_initial_stack_info(repo: &RepoState) -> StackInfo {
    let _timer = perf::ScopeTimer::new("load_initial_stack_info");
    detect_stacks(&repo.root, repo, true)
}

fn stack_notice(stack_info: &StackInfo) -> Option<&'static str> {
    match stack_info.detection_status {
        StackDetectionStatus::Ready => None,
        StackDetectionStatus::GraphiteUnavailable => {
            Some("Graphite unavailable; showing inferred local branch relationships.")
        }
        StackDetectionStatus::GraphiteParseFailed => {
            Some("Graphite stack parse failed; showing inferred local branch relationships.")
        }
    }
}

fn build_stack_view(
    repo: &RepoState,
    stack_info: &StackInfo,
    branch_name: &str,
) -> Option<StackView> {
    let stack = stack_info.stack_for_branch(branch_name)?;
    let branch_map: std::collections::HashMap<_, _> = repo
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

fn diff_preload_targets(
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

fn preload_branch_diff(
    root: &std::path::Path,
    branch: BranchInfo,
    options: DiffOptions,
) -> Result<PreloadedBranchDiff> {
    let diff = load_branch_diff(root, &branch, options)?;
    let highlighted_diff = SyntaxHighlighter::new().highlight_diff(&diff)?;
    Ok(PreloadedBranchDiff {
        diff,
        highlighted_diff,
    })
}

fn build_display_entries(
    repo: &RepoState,
    stack_info: &StackInfo,
    worktrees: &[WorktreeInfo],
    expanded_stacks: &HashSet<String>,
) -> Vec<BranchEntry> {
    let mut entries = Vec::new();
    let mut used_branches = std::collections::HashSet::new();
    let branch_map: std::collections::HashMap<_, _> = repo
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

    // Stacked branches
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
    let mut seen_standalone = std::collections::HashSet::new();
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

fn initial_expanded_stacks(repo: &RepoState, stack_info: &StackInfo) -> HashSet<String> {
    stack_info
        .stacks
        .iter()
        .filter(|stack| stack_contains_head(repo, stack))
        .map(|stack| stack.name.clone())
        .collect()
}

fn stack_contains_head(repo: &RepoState, stack: &stack::Stack) -> bool {
    stack.branches.iter().any(|branch_name| {
        repo.branches
            .iter()
            .any(|branch| branch.name == *branch_name && branch.is_head)
    })
}

fn stack_name_for_branch<'a>(stack_info: &'a StackInfo, branch_name: &str) -> Option<&'a str> {
    stack_info
        .stacks
        .iter()
        .find(|stack| stack.branches.iter().any(|name| name == branch_name))
        .map(|stack| stack.name.as_str())
}

fn ordered_branch_names(repo: &RepoState, stack_info: &StackInfo) -> Vec<String> {
    let mut branches = Vec::new();
    let mut used_branches = HashSet::new();

    for stack in &stack_info.stacks {
        for branch_name in &stack.branches {
            if repo.branches.iter().any(|branch| branch.name == *branch_name) {
                branches.push(branch_name.clone());
                used_branches.insert(branch_name.clone());
            }
        }
    }

    for branch_name in &stack_info.standalone {
        if repo.branches.iter().any(|branch| branch.name == *branch_name)
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

fn branch_for_diff(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::BranchInfo;
    use crate::stack::{Stack, StackInfo};
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn make_repo_at(root: PathBuf, branch_names: &[&str]) -> RepoState {
        RepoState {
            root,
            branches: branch_names.iter().map(|n| make_branch(n)).collect(),
        }
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("gl-{label}-{}-{nanos}", std::process::id()))
    }

    fn run_git(root: &std::path::Path, args: &[&str]) {
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

    fn make_commit_breakdown_repo() -> (PathBuf, RepoState) {
        let repo_root = unique_temp_dir("commit-breakdown");
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init", "-b", "main"]);
        run_git(&repo_root, &["config", "user.name", "GL Test"]);
        run_git(&repo_root, &["config", "user.email", "gl@example.com"]);

        fs::write(repo_root.join("notes.txt"), "base\n").unwrap();
        run_git(&repo_root, &["add", "notes.txt"]);
        run_git(&repo_root, &["commit", "-m", "initial"]);

        run_git(&repo_root, &["checkout", "-b", "a1"]);
        fs::write(repo_root.join("notes.txt"), "base\nfirst\n").unwrap();
        run_git(&repo_root, &["commit", "-am", "first change"]);

        fs::write(repo_root.join("notes.txt"), "base\nfirst\nsecond\n").unwrap();
        run_git(&repo_root, &["commit", "-am", "second change"]);

        let repo = refresh_repo(&repo_root).unwrap();
        (repo_root, repo)
    }

    fn empty_stacks() -> StackInfo {
        StackInfo {
            stacks: vec![],
            standalone: vec![],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        }
    }

    // --- build_display_entries ---

    #[test]
    fn display_entries_no_stacks_flat_list() {
        let repo = make_repo(&["feat-a", "feat-b", "main"]);
        let stacks = empty_stacks();
        let entries = build_display_entries(&repo, &stacks, &[], &HashSet::new());

        // No headers when no stacks
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
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };

        let entries = build_display_entries(
            &repo,
            &stacks,
            &[],
            &HashSet::from(["auth stack".to_string()]),
        );

        // Should have: header, auth-base, auth-ui, header(standalone), main
        let headers: Vec<_> = entries.iter().filter(|e| e.is_header()).collect();
        assert_eq!(headers.len(), 2); // "auth stack" + "standalone"

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
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };

        let entries = build_display_entries(
            &repo,
            &stacks,
            &[],
            &HashSet::from(["my stack".to_string()]),
        );

        // Check indentation increases
        let branch_entries: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                BranchEntry::Branch { indent, .. } => Some(*indent),
                _ => None,
            })
            .collect();
        // Stack branches get indent 1, 2, 3; standalone (main) gets 0
        assert_eq!(branch_entries[0], 1); // base
        assert_eq!(branch_entries[1], 2); // mid
        assert_eq!(branch_entries[2], 3); // top
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
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
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
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };

        let branch = branch_for_diff(&repo, &stacks, "stack-top").unwrap();
        assert_eq!(branch.base_ref.as_deref(), Some("stack-base"));
    }

    #[test]
    fn app_applies_lazy_stack_decorations_without_reordering_entries() {
        let repo = make_repo(&["alpha-top", "alpha-base", "main"]);
        let stack_info = StackInfo {
            stacks: vec![Stack {
                name: "alpha stack".into(),
                branches: vec!["alpha-base".into(), "alpha-top".into()],
            }],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::from([
                ("alpha-top".into(), "alpha-base".into()),
                ("alpha-base".into(), "main".into()),
            ]),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };

        let mut app = App::new_for_test(AppConfig::default(), repo, stack_info.clone());
        let before: Vec<_> = app
            .display_entries
            .iter()
            .map(|entry| entry.branch_name().to_string())
            .collect();

        app.stack_request_id += 1;
        let request_id = app.stack_request_id;
        let mut decorated = stack_info;
        decorated.stale_branches.insert("alpha-top".to_string());
        app.stack_result_tx
            .send(StackLoadResult {
                request_id,
                stack_info: decorated,
            })
            .unwrap();

        assert!(app.apply_pending_stack_updates());

        let after: Vec<_> = app
            .display_entries
            .iter()
            .map(|entry| entry.branch_name().to_string())
            .collect();
        assert_eq!(after, before);
    }

    #[test]
    fn initial_stack_info_builds_first_paint_shape_from_gt_output() {
        let _env_guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let repo_root = unique_temp_dir("startup-repo");
        fs::create_dir_all(&repo_root).unwrap();
        let fake_bin = unique_temp_dir("fake-gt-bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_gt = fake_bin.join("gt");
        fs::write(
            &fake_gt,
            "#!/bin/sh\nif [ \"$1\" = \"log\" ] && [ \"$2\" = \"short\" ] && [ \"$3\" = \"--no-interactive\" ]; then\ncat <<'EOF'\n◉    alpha-top\n◯    alpha-base\n│ ◯  beta-top\n│ ◯  beta-base\n◯─┘  main\nEOF\nelse\nexit 1\nfi\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&fake_gt).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_gt, permissions).unwrap();

        let original_path = std::env::var_os("PATH");
        let mut path_entries = vec![fake_bin.clone()];
        path_entries.extend(std::env::split_paths(
            &original_path.clone().unwrap_or_default(),
        ));
        let joined_path = std::env::join_paths(path_entries).unwrap();
        std::env::set_var("PATH", &joined_path);

        let mut runtime_repo = make_repo_at(
            repo_root.clone(),
            &["alpha-top", "alpha-base", "beta-top", "beta-base", "main"],
        );
        for branch in &mut runtime_repo.branches {
            branch.object_id.push_str("-new");
        }
        runtime_repo.branches[0].is_head = true;

        let startup_stack_info = load_initial_stack_info(&runtime_repo);
        let app = App::new_for_test(AppConfig::default(), runtime_repo, startup_stack_info);
        let labels_and_branches: Vec<_> = app
            .display_entries
            .iter()
            .map(|entry| match entry {
                BranchEntry::Header { label, .. } => format!("header:{label}"),
                BranchEntry::Branch { branch_name, .. } => format!("branch:{branch_name}"),
            })
            .collect();

        assert_eq!(
            labels_and_branches,
            vec![
                "header:alpha-base stack",
                "branch:alpha-base",
                "branch:alpha-top",
                "header:beta-base stack",
                "header:standalone",
                "branch:main",
            ]
        );

        if let Some(original_path) = original_path {
            std::env::set_var("PATH", original_path);
        } else {
            std::env::remove_var("PATH");
        }
    }

    // --- selection navigation helpers ---

    fn make_test_entries() -> Vec<BranchEntry> {
        vec![
            BranchEntry::Header {
                label: "stack A".into(),
                expanded: Some(true),
            },
            BranchEntry::Branch {
                branch_name: "a1".into(),
                is_head: false,
                commit_count: 1,
                ahead: 0,
                behind: 0,
                has_upstream: false,
                indent: 1,
                stale: false,
                worktree_label: None,
            },
            BranchEntry::Branch {
                branch_name: "a2".into(),
                is_head: false,
                commit_count: 1,
                ahead: 0,
                behind: 0,
                has_upstream: false,
                indent: 2,
                stale: false,
                worktree_label: None,
            },
            BranchEntry::Header {
                label: "stack B".into(),
                expanded: Some(false),
            },
            BranchEntry::Branch {
                branch_name: "b1".into(),
                is_head: false,
                commit_count: 1,
                ahead: 0,
                behind: 0,
                has_upstream: false,
                indent: 1,
                stale: false,
                worktree_label: None,
            },
            BranchEntry::Header {
                label: "standalone".into(),
                expanded: None,
            },
            BranchEntry::Branch {
                branch_name: "main".into(),
                is_head: true,
                commit_count: 0,
                ahead: 0,
                behind: 0,
                has_upstream: false,
                indent: 0,
                stale: false,
                worktree_label: None,
            },
        ]
    }

    fn make_test_app(entries: Vec<BranchEntry>) -> App {
        let (stack_result_tx, stack_result_rx) = mpsc::channel();
        let (commit_count_result_tx, commit_count_result_rx) = mpsc::channel();
        let (diff_preload_result_tx, diff_preload_result_rx) = mpsc::channel();
        let (watch_event_tx, watch_event_rx) = mpsc::channel();
        let mut repo = make_repo_at(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            &["a1", "a2", "b1", "main"],
        );
        repo.branches[3].is_head = true;
        App {
            config: AppConfig::default(),
            repo,
            stack_info: empty_stacks(),
            expanded_stacks: HashSet::new(),
            manual_stack_states: HashMap::new(),
            auto_expanded_stack: None,
            display_entries: entries,
            selected_index: 0,
            show_stack_view: false,
            show_graph_view: false,
            show_worktree_view: false,
            detail_kind: None,
            branch_diff: None,
            highlighted_diff: None,
            diff_scroll: 0,
            show_help: false,
            focus: FocusedPane::BranchList,
            search_mode: false,
            search_input: String::new(),
            search_matches: Vec::new(),
            search_cursor: 0,
            command_mode: false,
            command_input: String::new(),
            stack_result_tx,
            stack_result_rx,
            stack_request_id: 0,
            commit_count_result_tx,
            commit_count_result_rx,
            commit_count_request_id: 0,
            diff_preload_result_tx,
            diff_preload_result_rx,
            diff_preload_request_id: 0,
            diff_preload_started: false,
            preloaded_diffs: HashMap::new(),
            diff_preload_failures: HashMap::new(),
            pending_diff_preloads: std::collections::HashSet::new(),
            branch_detail: None,
            graph_commits: Vec::new(),
            graph_selected_index: 0,
            worktrees: Vec::new(),
            worktree_selected_index: 0,
            diff_view: DiffViewMode::Unified,
            ignore_whitespace: false,
            _watch_event_tx: watch_event_tx,
            watch_event_rx,
            _repo_watcher: None,
            watch_notice: None,
            user_notice: None,
        }
    }

    #[test]
    fn parse_cli_args_supports_help_version_and_repo_path() {
        let cli = parse_cli_args(vec!["--version".to_string(), "/tmp/repo".to_string()]).unwrap();
        assert!(cli.show_version);
        assert_eq!(cli.repo_path, Some(PathBuf::from("/tmp/repo")));

        let cli = parse_cli_args(vec!["--help".to_string()]).unwrap();
        assert!(cli.show_help);
    }

    #[test]
    fn parse_cli_args_supports_color_scheme_override() {
        let cli = parse_cli_args(vec![
            "--color-scheme".to_string(),
            "violet".to_string(),
            "/tmp/repo".to_string(),
        ])
        .unwrap();
        assert_eq!(cli.color_scheme, Some(ColorScheme::Violet));
        assert_eq!(cli.repo_path, Some(PathBuf::from("/tmp/repo")));

        let cli = parse_cli_args(vec!["--color-scheme=teal".to_string()]).unwrap();
        assert_eq!(cli.color_scheme, Some(ColorScheme::Teal));
    }

    #[test]
    fn parse_cli_args_rejects_unknown_flags() {
        let error = parse_cli_args(vec!["--wat".to_string()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown argument"));
    }

    #[test]
    fn parse_cli_args_rejects_invalid_or_missing_color_scheme() {
        let invalid = parse_cli_args(vec!["--color-scheme".to_string(), "banana".to_string()])
            .unwrap_err()
            .to_string();
        assert!(invalid.contains("unknown color scheme"));
        assert!(invalid.contains("ocean"));

        let missing = parse_cli_args(vec!["--color-scheme".to_string()])
            .unwrap_err()
            .to_string();
        assert!(missing.contains("missing value for --color-scheme"));
    }

    #[test]
    fn help_text_lists_color_scheme_flag_and_values() {
        let help = help_text();
        assert!(help.contains("--color-scheme <scheme>"));
        assert!(help.contains("ocean, forest, amber, violet, rose, teal"));
    }

    #[test]
    fn move_selection_skips_headers_in_branch_list() {
        let mut app = make_test_app(make_test_entries());
        app.jump_to_first_branch();

        app.move_selection(1);
        assert_eq!(app.selected_index, 2);

        app.move_selection(1);
        assert_eq!(app.selected_index, 4);

        app.move_selection(1);
        assert_eq!(app.selected_index, 6);
    }

    #[test]
    fn non_current_stacks_start_folded() {
        let mut repo = make_repo(&["alpha-base", "alpha-top", "beta-base", "beta-top", "main"]);
        repo.branches[1].is_head = true;
        let stacks = StackInfo {
            stacks: vec![
                Stack {
                    name: "alpha stack".into(),
                    branches: vec!["alpha-base".into(), "alpha-top".into()],
                },
                Stack {
                    name: "beta stack".into(),
                    branches: vec!["beta-base".into(), "beta-top".into()],
                },
            ],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };

        let app = App::new_for_test(AppConfig::default(), repo, stacks);
        let labels_and_branches: Vec<_> = app
            .display_entries
            .iter()
            .map(|entry| match entry {
                BranchEntry::Header { label, .. } => format!("header:{label}"),
                BranchEntry::Branch { branch_name, .. } => format!("branch:{branch_name}"),
            })
            .collect();

        assert_eq!(
            labels_and_branches,
            vec![
                "header:alpha stack",
                "branch:alpha-base",
                "branch:alpha-top",
                "header:beta stack",
                "header:standalone",
                "branch:main",
            ]
        );
    }

    #[test]
    fn move_selection_expands_next_collapsed_stack() {
        let mut repo = make_repo(&["a1", "a2", "b1", "main"]);
        repo.branches[1].is_head = true;
        let stack_info = StackInfo {
            stacks: vec![
                Stack {
                    name: "stack A".into(),
                    branches: vec!["a1".into(), "a2".into()],
                },
                Stack {
                    name: "stack B".into(),
                    branches: vec!["b1".into()],
                },
            ],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };
        let mut app = App::new_for_test(AppConfig::default(), repo, stack_info);
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "a2")
            .unwrap();

        app.move_selection(1);

        assert_eq!(app.selected_branch_name(), Some("b1"));
        let labels_and_branches: Vec<_> = app
            .display_entries
            .iter()
            .map(|entry| match entry {
                BranchEntry::Header { label, .. } => format!("header:{label}"),
                BranchEntry::Branch { branch_name, .. } => format!("branch:{branch_name}"),
            })
            .collect();
        assert!(labels_and_branches.contains(&"branch:b1".to_string()));
    }

    #[test]
    fn move_selection_refolds_transient_stack_after_leaving_it() {
        let mut repo = make_repo(&["a1", "a2", "b1", "main"]);
        repo.branches[3].is_head = true;
        let stack_info = StackInfo {
            stacks: vec![
                Stack {
                    name: "stack A".into(),
                    branches: vec!["a1".into(), "a2".into()],
                },
                Stack {
                    name: "stack B".into(),
                    branches: vec!["b1".into()],
                },
            ],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };
        let mut app = App::new_for_test(AppConfig::default(), repo, stack_info);
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "main")
            .unwrap();

        app.move_selection(-1);
        assert_eq!(app.selected_branch_name(), Some("b1"));
        assert!(app.expanded_stacks.contains("stack B"));

        app.move_selection(1);
        assert_eq!(app.selected_branch_name(), Some("main"));
        assert!(!app.expanded_stacks.contains("stack B"));
        assert!(
            app.display_entries
                .iter()
                .all(|entry| entry.is_header() || entry.branch_name() != "b1")
        );
    }

    #[test]
    fn move_selection_expands_previous_collapsed_stack() {
        let mut repo = make_repo(&["a1", "a2", "b1", "main"]);
        repo.branches[3].is_head = true;
        let stack_info = StackInfo {
            stacks: vec![
                Stack {
                    name: "stack A".into(),
                    branches: vec!["a1".into(), "a2".into()],
                },
                Stack {
                    name: "stack B".into(),
                    branches: vec!["b1".into()],
                },
            ],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };
        let mut app = App::new_for_test(AppConfig::default(), repo, stack_info);
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "main")
            .unwrap();

        app.move_selection(-1);

        assert_eq!(app.selected_branch_name(), Some("b1"));
    }

    #[test]
    fn manual_fold_prevents_implicit_scroll_expansion() {
        let mut repo = make_repo(&["a1", "a2", "b1", "main"]);
        repo.branches[1].is_head = true;
        let stack_info = StackInfo {
            stacks: vec![
                Stack {
                    name: "stack A".into(),
                    branches: vec!["a1".into(), "a2".into()],
                },
                Stack {
                    name: "stack B".into(),
                    branches: vec!["b1".into()],
                },
            ],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };
        let mut app = App::new_for_test(AppConfig::default(), repo, stack_info);
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "a2")
            .unwrap();

        app.fold_selected_stack_manually();
        assert_eq!(
            app.manual_stack_states.get("stack A").copied(),
            Some(false)
        );
        assert!(
            app.display_entries
                .iter()
                .all(|entry| entry.is_header() || entry.branch_name() != "a1")
        );

        app.move_selection(1);

        assert_eq!(app.selected_branch_name(), Some("b1"));
        assert!(app.expanded_stacks.contains("stack B"));
        assert!(
            app.display_entries
                .iter()
                .all(|entry| entry.is_header() || entry.branch_name() != "a1")
        );

        app.move_selection(1);

        assert_eq!(app.selected_branch_name(), Some("main"));
        assert!(!app.expanded_stacks.contains("stack B"));
        assert!(
            app.display_entries
                .iter()
                .all(|entry| entry.is_header() || entry.branch_name() != "b1")
        );
    }

    #[test]
    fn manual_unfold_keeps_stack_open_after_sequential_navigation_leaves() {
        let mut repo = make_repo(&["a1", "a2", "b1", "main"]);
        repo.branches[3].is_head = true;
        let stack_info = StackInfo {
            stacks: vec![
                Stack {
                    name: "stack A".into(),
                    branches: vec!["a1".into(), "a2".into()],
                },
                Stack {
                    name: "stack B".into(),
                    branches: vec!["b1".into()],
                },
            ],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };
        let mut app = App::new_for_test(AppConfig::default(), repo, stack_info);
        let stack_b_header = app
            .display_entries
            .iter()
            .position(|entry| matches!(entry, BranchEntry::Header { label, .. } if label == "stack B"))
            .unwrap();
        app.selected_index = stack_b_header;

        app.unfold_selected_stack_manually();
        assert_eq!(app.selected_branch_name(), Some("b1"));
        assert_eq!(app.manual_stack_states.get("stack B").copied(), Some(true));

        app.move_selection(1);

        assert_eq!(app.selected_branch_name(), Some("main"));
        assert!(app.expanded_stacks.contains("stack B"));
        assert!(
            app.display_entries
                .iter()
                .any(|entry| !entry.is_header() && entry.branch_name() == "b1")
        );
    }

    #[test]
    fn move_selection_clamps_at_start_of_branch_list() {
        let mut app = make_test_app(make_test_entries());

        app.jump_to_first_branch();
        app.move_selection(-1);

        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn move_selection_clamps_at_end_of_branch_list() {
        let mut app = make_test_app(make_test_entries());

        app.jump_to_last_branch();
        app.move_selection(1);

        assert_eq!(app.selected_index, 6);
    }

    #[test]
    fn jump_stack_group_moves_to_next_group_start() {
        let mut app = make_test_app(make_test_entries());

        app.jump_to_first_branch();
        app.jump_stack_group(1);
        assert_eq!(app.selected_index, 4);

        app.jump_stack_group(1);
        assert_eq!(app.selected_index, 6);

        app.jump_stack_group(-1);
        assert_eq!(app.selected_index, 4);
    }

    #[test]
    fn jump_stack_group_does_not_fold_or_unfold_stacks() {
        let mut repo = make_repo(&["a1", "a2", "b1", "main"]);
        repo.branches[3].is_head = true;
        let stack_info = StackInfo {
            stacks: vec![
                Stack {
                    name: "stack A".into(),
                    branches: vec!["a1".into(), "a2".into()],
                },
                Stack {
                    name: "stack B".into(),
                    branches: vec!["b1".into()],
                },
            ],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };
        let mut app = App::new_for_test(AppConfig::default(), repo, stack_info);
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "main")
            .unwrap();

        app.move_selection(-1);
        assert!(app.expanded_stacks.contains("stack B"));

        app.jump_stack_group(-1);

        assert!(app.expanded_stacks.contains("stack B"));
        assert!(
            app.display_entries
                .iter()
                .any(|entry| !entry.is_header() && entry.branch_name() == "b1")
        );
    }

    #[test]
    fn jump_to_first_branch_selects_first_branch_entry() {
        let mut app = make_test_app(make_test_entries());
        app.selected_index = 4;

        app.jump_to_first_branch();

        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn jump_to_last_branch_selects_last_branch_entry() {
        let mut app = make_test_app(make_test_entries());
        app.selected_index = 1;

        app.jump_to_last_branch();

        assert_eq!(app.selected_index, 6);
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
            stale_branches: std::collections::HashSet::from(["auth-ui".into()]),
            detection_status: StackDetectionStatus::Ready,
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
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };

        let view = build_stack_view(&repo, &stacks, "auth-base").unwrap();
        assert_eq!(view.parent_branch.as_deref(), Some("main"));
        assert_eq!(view.child_branch.as_deref(), Some("auth-ui"));
    }

    #[test]
    fn toggle_stack_view_only_opens_for_branches_in_a_stack() {
        let mut repo = make_repo(&["auth-base", "auth-ui", "main"]);
        repo.branches[0].is_head = true;
        let stack_info = StackInfo {
            stacks: vec![Stack {
                name: "auth stack".into(),
                branches: vec!["auth-base".into(), "auth-ui".into()],
            }],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::from([
                ("auth-base".into(), "main".into()),
                ("auth-ui".into(), "auth-base".into()),
            ]),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };
        let mut app = App::new_for_test(AppConfig::default(), repo, stack_info);
        app.jump_to_first_branch();

        app.toggle_stack_view();
        assert!(app.show_stack_view);
        assert_eq!(
            app.current_stack_view()
                .as_ref()
                .map(|view| view.selected_branch.as_str()),
            Some("auth-base")
        );

        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "main")
            .unwrap();
        app.show_stack_view = false;
        app.toggle_stack_view();
        assert!(!app.show_stack_view);
    }

    #[test]
    fn refresh_closes_stack_view_when_selected_branch_is_no_longer_stacked() {
        let mut app = make_test_app(make_test_entries());
        app.stack_info = StackInfo {
            stacks: vec![Stack {
                name: "stack A".into(),
                branches: vec!["a1".into(), "a2".into()],
            }],
            standalone: vec!["b1".into(), "main".into()],
            branch_to_parent: HashMap::from([
                ("a1".into(), "main".into()),
                ("a2".into(), "a1".into()),
            ]),
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };
        app.expanded_stacks.insert("stack A".into());
        app.display_entries =
            build_display_entries(&app.repo, &app.stack_info, &[], &app.expanded_stacks);
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "a2")
            .unwrap();
        app.show_stack_view = true;

        app.stack_info = empty_stacks();
        if app.show_stack_view && app.current_stack_view().is_none() {
            app.show_stack_view = false;
        }

        assert!(!app.show_stack_view);
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
            stale_branches: std::collections::HashSet::new(),
            detection_status: StackDetectionStatus::Ready,
        };
        let entries = build_display_entries(
            &repo,
            &stacks,
            &[],
            &HashSet::from(["stack".to_string()]),
        );

        let targets = diff_preload_targets(&repo, &stacks, &entries);
        let names: Vec<_> = targets.iter().map(|branch| branch.name.as_str()).collect();
        assert_eq!(names, vec!["base", "top", "main"]);
        assert_eq!(targets[1].base_ref.as_deref(), Some("base"));
    }

    #[test]
    fn wait_for_preloaded_diff_promotes_completed_async_result_into_cache() {
        let mut app = make_test_app(make_test_entries());
        app.diff_preload_request_id = 1;
        app.pending_diff_preloads.insert("a1".into());
        app.diff_preload_result_tx
            .send(DiffPreloadResult {
                request_id: 1,
                branch_name: "a1".into(),
                preloaded: Ok(PreloadedBranchDiff {
                    diff: BranchDiff {
                        branch_name: "a1".into(),
                        base_ref: Some("main".into()),
                        title: None,
                        ignore_whitespace: false,
                        lines: vec![],
                        file_positions: vec![],
                    },
                    highlighted_diff: vec![Line::from("cached")],
                }),
            })
            .unwrap();

        let preloaded = app.wait_for_preloaded_diff("a1").unwrap();
        assert_eq!(preloaded.diff.branch_name, "a1");
        assert!(app.preloaded_diffs.contains_key("a1"));
        assert!(!app.pending_diff_preloads.contains("a1"));
    }

    #[test]
    fn open_selected_branch_uses_preloaded_diff_without_git_roundtrip() {
        let (repo_root, repo) = make_commit_breakdown_repo();
        let mut app = App::new_for_test(AppConfig::default(), repo, empty_stacks());
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "a1")
            .unwrap();
        app.preloaded_diffs.insert(
            "a1".into(),
            PreloadedBranchDiff {
                diff: BranchDiff {
                    branch_name: "a1".into(),
                    base_ref: Some("main".into()),
                    title: None,
                    ignore_whitespace: false,
                    lines: vec![],
                    file_positions: vec![],
                },
                highlighted_diff: vec![Line::from("preloaded")],
            },
        );

        app.open_selected_branch().unwrap();

        assert_eq!(
            app.branch_diff
                .as_ref()
                .map(|diff| diff.branch_name.as_str()),
            Some("a1")
        );
        assert_eq!(
            app.highlighted_diff
                .as_ref()
                .and_then(|lines| lines.first())
                .map(|line| line.spans[0].content.as_ref()),
            Some("preloaded")
        );
        assert_eq!(app.detail_kind, Some(DetailKind::BranchDiff));
        assert_eq!(
            app.branch_detail
                .as_ref()
                .map(|detail| detail.commits.len()),
            Some(2)
        );
        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn branch_detail_commit_breakdown_opens_commit_diff_and_restores_branch_diff() {
        let (repo_root, repo) = make_commit_breakdown_repo();
        let mut app = App::new_for_test(AppConfig::default(), repo, empty_stacks());
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "a1")
            .unwrap();

        app.open_selected_branch().unwrap();
        assert_eq!(
            app.branch_detail
                .as_ref()
                .map(|detail| detail.commits.len()),
            Some(2)
        );

        app.handle_detail_keys(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert!(app.commit_list_is_open());

        app.handle_diff_keys(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(
            app.branch_detail.as_ref().map(|detail| &detail.diff_mode),
            Some(BranchDetailDiffMode::Commit { .. })
        ));
        let title = app
            .branch_diff
            .as_ref()
            .and_then(|diff| diff.title.as_deref())
            .unwrap();
        assert!(title.starts_with("a1 @ "));
        assert!(title.contains("second change"));

        app.handle_diff_keys(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.branch_diff
                .as_ref()
                .map(|diff| diff.branch_name.as_str()),
            Some("a1")
        );
        assert!(matches!(
            app.branch_detail.as_ref().map(|detail| &detail.diff_mode),
            Some(BranchDetailDiffMode::Branch)
        ));
        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn branch_detail_info_overlay_dismisses_on_next_key() {
        let (repo_root, repo) = make_commit_breakdown_repo();
        let mut app = App::new_for_test(AppConfig::default(), repo, empty_stacks());
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "a1")
            .unwrap();

        app.open_selected_branch().unwrap();
        app.handle_diff_keys(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .unwrap();
        let overlay = app
            .branch_detail
            .as_ref()
            .and_then(|detail| detail.info_overlay.as_ref())
            .cloned()
            .unwrap();
        assert!(overlay
            .iter()
            .any(|line| line.contains("Base branch: main")));
        assert!(overlay.iter().any(|line| line.contains("Worktree:")));

        app.handle_detail_keys(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
            .unwrap();
        assert!(app
            .branch_detail
            .as_ref()
            .and_then(|detail| detail.info_overlay.as_ref())
            .is_none());
        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn open_selected_status_only_opens_for_head_branch() {
        let mut app = make_test_app(make_test_entries());
        app.selected_index = 1;
        app.open_selected_status().unwrap();
        assert!(app.branch_diff.is_none());

        app.selected_index = 6;
        app.open_selected_status().unwrap();
        assert_eq!(app.detail_kind, Some(DetailKind::Status));
        assert_eq!(
            app.branch_diff
                .as_ref()
                .map(|diff| diff.base_ref.as_deref()),
            Some(Some("working tree"))
        );
    }

    #[test]
    fn shift_s_opens_status_instead_of_toggling_stack_view() {
        let mut app = make_test_app(make_test_entries());
        app.selected_index = 6;

        app.handle_branch_list_keys(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT))
            .unwrap();

        assert_eq!(app.detail_kind, Some(DetailKind::Status));
        assert!(!app.show_stack_view);
    }

    #[test]
    fn execute_command_branch_jumps_to_named_branch() {
        let mut app = make_test_app(make_test_entries());
        assert!(!app.jump_to_branch("missing"));
        assert!(!app.execute_command("branch missing").unwrap());
        assert!(app
            .user_notice
            .as_deref()
            .is_some_and(|message| message.contains("No branch matched")));

        assert!(!app.execute_command("branch b1").unwrap());
        assert_eq!(app.selected_branch_name(), Some("b1"));
    }

    #[test]
    fn execute_command_search_selects_first_matching_branch() {
        let mut app = make_test_app(make_test_entries());
        assert!(!app.execute_command("search a").unwrap());
        assert_eq!(app.selected_branch_name(), Some("a1"));
        assert!(app
            .user_notice
            .as_deref()
            .is_some_and(|message| message.contains("Matched branch search")));
    }

    #[test]
    fn execute_command_q_requests_quit() {
        let mut app = make_test_app(make_test_entries());
        assert!(app.execute_command("q").unwrap());
    }

    #[test]
    fn filesystem_refresh_updates_open_status_view_after_worktree_change() {
        let repo_root = unique_temp_dir("watch-refresh");
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init", "-b", "main"]);
        run_git(&repo_root, &["config", "user.name", "GL Test"]);
        run_git(&repo_root, &["config", "user.email", "gl@example.com"]);

        fs::write(repo_root.join("notes.txt"), "base\n").unwrap();
        run_git(&repo_root, &["add", "notes.txt"]);
        run_git(&repo_root, &["commit", "-m", "initial"]);

        let repo = refresh_repo(&repo_root).unwrap();
        let mut app = App::new_for_test(AppConfig::default(), repo, empty_stacks());
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "main")
            .unwrap();
        app.open_selected_status().unwrap();
        assert!(app
            .branch_diff
            .as_ref()
            .is_some_and(|diff| diff.lines.iter().any(|line| line.text.contains("clean"))));

        fs::write(repo_root.join("notes.txt"), "base\nchanged\n").unwrap();
        app._watch_event_tx
            .send(WatchMessage::RefreshRequested)
            .unwrap();

        assert!(app.apply_pending_watch_events().unwrap());
        assert!(app.branch_diff.as_ref().is_some_and(|diff| {
            diff.lines
                .iter()
                .any(|line| line.text.contains("0 staged, 1 unstaged, 0 untracked"))
                && diff
                    .lines
                    .iter()
                    .any(|line| line.text.contains("notes.txt"))
        }));

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn toggle_diff_view_switches_between_unified_and_side_by_side() {
        let mut app = make_test_app(make_test_entries());
        assert_eq!(app.diff_view, DiffViewMode::Unified);
        app.toggle_diff_view();
        assert_eq!(app.diff_view, DiffViewMode::SideBySide);
        app.toggle_diff_view();
        assert_eq!(app.diff_view, DiffViewMode::Unified);
    }

    #[test]
    fn toggle_whitespace_mode_reloads_branch_diff_with_ignore_all_space() {
        let repo_root = unique_temp_dir("toggle-whitespace");
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init", "-b", "main"]);
        run_git(&repo_root, &["config", "user.name", "GL Test"]);
        run_git(&repo_root, &["config", "user.email", "gl@example.com"]);

        fs::write(repo_root.join("notes.txt"), "base\n").unwrap();
        run_git(&repo_root, &["add", "notes.txt"]);
        run_git(&repo_root, &["commit", "-m", "initial"]);
        run_git(&repo_root, &["checkout", "-b", "feature"]);
        fs::write(repo_root.join("notes.txt"), "base \n").unwrap();
        run_git(&repo_root, &["commit", "-am", "whitespace only"]);

        let repo = refresh_repo(&repo_root).unwrap();
        let mut app = App::new_for_test(AppConfig::default(), repo, empty_stacks());
        app.selected_index = app
            .display_entries
            .iter()
            .position(|entry| !entry.is_header() && entry.branch_name() == "feature")
            .unwrap();

        app.open_selected_branch().unwrap();
        assert!(app
            .branch_diff
            .as_ref()
            .is_some_and(|diff| diff.lines.iter().any(|line| matches!(
                line.kind,
                crate::git::DiffLineKind::Add | crate::git::DiffLineKind::Del
            ))));

        app.toggle_whitespace_mode().unwrap();
        assert!(app.ignore_whitespace);
        assert!(app
            .branch_diff
            .as_ref()
            .is_some_and(|diff| diff.ignore_whitespace
                && diff
                    .lines
                    .iter()
                    .any(|line| line.text.contains("identical"))));

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn toggle_graph_view_loads_commit_graph_and_focuses_graph_pane() {
        let (repo_root, repo) = make_commit_breakdown_repo();
        let mut app = App::new_for_test(AppConfig::default(), repo, empty_stacks());

        app.toggle_graph_view().unwrap();

        assert!(app.show_graph_view);
        assert_eq!(app.focus, FocusedPane::Diff);
        assert!(!app.graph_commits.is_empty());

        fs::remove_dir_all(repo_root).unwrap();
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

    #[test]
    fn switch_to_selected_worktree_updates_active_repo_root() {
        let repo_root = unique_temp_dir("worktree-switch");
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init", "-b", "main"]);
        run_git(&repo_root, &["config", "user.name", "GL Test"]);
        run_git(&repo_root, &["config", "user.email", "gl@example.com"]);
        fs::write(repo_root.join("notes.txt"), "base\n").unwrap();
        run_git(&repo_root, &["add", "notes.txt"]);
        run_git(&repo_root, &["commit", "-m", "initial"]);
        run_git(&repo_root, &["branch", "feature"]);

        let feature_worktree = unique_temp_dir("feature-wt");
        run_git(
            &repo_root,
            &[
                "worktree",
                "add",
                feature_worktree.to_str().unwrap(),
                "feature",
            ],
        );

        let repo = refresh_repo(&repo_root).unwrap();
        let mut app = App::new_for_test(AppConfig::default(), repo, empty_stacks());
        app.worktrees = load_worktrees(&repo_root).unwrap();
        app.worktree_selected_index = app
            .worktrees
            .iter()
            .position(|worktree| worktree.branch.as_deref() == Some("feature"))
            .unwrap();

        app.switch_to_selected_worktree().unwrap();

        assert_eq!(
            app.repo.root.canonicalize().unwrap(),
            feature_worktree.canonicalize().unwrap()
        );
        assert!(app
            .worktrees
            .iter()
            .any(|worktree| worktree.is_active && worktree.branch.as_deref() == Some("feature")));

        let active_root = app.repo.root.clone();
        drop(app);
        fs::remove_dir_all(repo_root).unwrap();
        fs::remove_dir_all(active_root).ok();
    }

    #[test]
    fn open_selected_graph_commit_branch_opens_branch_detail() {
        let (repo_root, repo) = make_commit_breakdown_repo();
        let mut app = App::new_for_test(AppConfig::default(), repo, empty_stacks());
        app.toggle_graph_view().unwrap();
        app.graph_selected_index = app
            .graph_commits
            .iter()
            .position(|commit| commit.primary_branch.as_deref() == Some("a1"))
            .unwrap();

        app.open_selected_graph_commit_branch().unwrap();

        assert_eq!(app.detail_kind, Some(DetailKind::BranchDiff));
        assert_eq!(
            app.branch_detail
                .as_ref()
                .map(|detail| detail.branch_name.as_str()),
            Some("a1")
        );

        fs::remove_dir_all(repo_root).unwrap();
    }
}
