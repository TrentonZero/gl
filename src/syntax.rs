use crate::git::{BranchDiff, DiffLine, DiffLineKind};
use crate::perf;
use anyhow::Result;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::collections::HashMap;
use syntect::{
    easy::HighlightLines,
    highlighting::{Color as SyntectColor, FontStyle, Style as SyntectStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileCacheKey {
    path: String,
    lines: Vec<(DiffLineKind, String)>,
}

pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
    file_cache: HashMap<FileCacheKey, Vec<Line<'static>>>,
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        let _timer = perf::ScopeTimer::new("SyntaxHighlighter::new");
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let themes = ThemeSet::load_defaults();
        let theme = themes
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .or_else(|| themes.themes.values().next().cloned())
            .unwrap_or_default();

        Self {
            syntax_set,
            theme,
            file_cache: HashMap::new(),
        }
    }

    pub fn highlight_diff(&mut self, diff: &BranchDiff) -> Result<Vec<Line<'static>>> {
        let _timer = perf::ScopeTimer::new(format!(
            "highlight_diff branch={} lines={}",
            diff.branch_name,
            diff.lines.len()
        ));
        let mut rendered = Vec::with_capacity(diff.lines.len());
        let mut index = 0usize;

        while index < diff.lines.len() {
            let line = &diff.lines[index];
            match line.kind {
                DiffLineKind::File | DiffLineKind::Hunk | DiffLineKind::Meta => {
                    rendered.push(render_plain_line(index, line));
                    index += 1;
                }
                DiffLineKind::Context | DiffLineKind::Add | DiffLineKind::Del => {
                    let Some(path) = line.file_path.clone() else {
                        rendered.push(render_plain_line(index, line));
                        index += 1;
                        continue;
                    };

                    let start = index;
                    while index < diff.lines.len() {
                        let candidate = &diff.lines[index];
                        let same_file = candidate.file_path.as_ref() == Some(&path);
                        let is_code = matches!(
                            candidate.kind,
                            DiffLineKind::Context | DiffLineKind::Add | DiffLineKind::Del
                        );
                        if !same_file || !is_code {
                            break;
                        }
                        index += 1;
                    }

                    let file_lines = self.highlight_file_block(path, &diff.lines[start..index])?;
                    rendered.extend(file_lines);
                }
            }
        }

        Ok(rendered)
    }

    fn highlight_file_block(
        &mut self,
        path: String,
        lines: &[DiffLine],
    ) -> Result<Vec<Line<'static>>> {
        let key = FileCacheKey {
            path: path.clone(),
            lines: lines
                .iter()
                .map(|line| (line.kind, line.text.clone()))
                .collect(),
        };

        if let Some(cached) = self.file_cache.get(&key) {
            return Ok(cached.clone());
        }

        let syntax = self
            .syntax_set
            .find_syntax_for_file(&path)?
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        let mut highlighter = HighlightLines::new(syntax, &self.theme);

        let mut rendered = Vec::with_capacity(lines.len());
        for (offset, line) in lines.iter().enumerate() {
            let code = strip_diff_prefix(&line.text);
            let ranges = highlighter.highlight_line(code, &self.syntax_set)?;
            let mut spans = vec![
                Span::styled(
                    format!("{:>4} ", offset + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(diff_prefix(line.kind), diff_prefix_style(line.kind)),
            ];

            if code.is_empty() {
                spans.push(Span::styled(
                    " ",
                    tinted_style(default_syntect_style(), line.kind),
                ));
            } else {
                for (style, segment) in ranges {
                    if segment.is_empty() {
                        continue;
                    }
                    spans.push(Span::styled(
                        segment.to_string(),
                        tinted_style(style, line.kind),
                    ));
                }
            }

            rendered.push(Line::from(spans));
        }

        self.file_cache.insert(key, rendered.clone());
        Ok(rendered)
    }
}

fn render_plain_line(index: usize, line: &DiffLine) -> Line<'static> {
    let (fg, bg, bold) = match line.kind {
        DiffLineKind::File => (Color::Magenta, Color::Reset, true),
        DiffLineKind::Hunk => (Color::Cyan, Color::Reset, false),
        DiffLineKind::Context => (Color::Gray, Color::Reset, false),
        DiffLineKind::Add => (Color::Green, Color::Rgb(31, 53, 31), false),
        DiffLineKind::Del => (Color::Red, Color::Rgb(59, 22, 22), false),
        DiffLineKind::Meta => (Color::Yellow, Color::Reset, false),
    };

    let mut style = Style::default().fg(fg).bg(bg);
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }

    let gutter = if matches!(line.kind, DiffLineKind::File | DiffLineKind::Hunk) {
        "    ".to_string()
    } else {
        format!("{:>4}", index + 1)
    };

    Line::from(vec![
        Span::styled(format!("{gutter} "), Style::default().fg(Color::DarkGray)),
        Span::styled(line.text.clone(), style),
    ])
}

fn strip_diff_prefix(text: &str) -> &str {
    match text.chars().next() {
        Some('+') | Some('-') | Some(' ') => &text[1..],
        _ => text,
    }
}

fn diff_prefix(kind: DiffLineKind) -> &'static str {
    match kind {
        DiffLineKind::Add => "+",
        DiffLineKind::Del => "-",
        _ => " ",
    }
}

fn diff_prefix_style(kind: DiffLineKind) -> Style {
    match kind {
        DiffLineKind::Add => Style::default()
            .fg(Color::Green)
            .bg(Color::Rgb(31, 53, 31))
            .add_modifier(Modifier::BOLD),
        DiffLineKind::Del => Style::default()
            .fg(Color::Red)
            .bg(Color::Rgb(59, 22, 22))
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(Color::Gray),
    }
}

fn tinted_style(style: SyntectStyle, kind: DiffLineKind) -> Style {
    let mut rendered = Style::default().fg(to_color(style.foreground));

    if style.font_style.contains(FontStyle::BOLD) {
        rendered = rendered.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        rendered = rendered.add_modifier(Modifier::ITALIC);
    }

    match kind {
        DiffLineKind::Add => rendered.bg(Color::Rgb(31, 53, 31)),
        DiffLineKind::Del => rendered.bg(Color::Rgb(59, 22, 22)),
        _ => rendered,
    }
}

fn to_color(color: SyntectColor) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

fn default_syntect_style() -> SyntectStyle {
    SyntectStyle {
        foreground: SyntectColor {
            r: 192,
            g: 202,
            b: 245,
            a: 0xFF,
        },
        background: SyntectColor {
            r: 26,
            g: 27,
            b: 38,
            a: 0xFF,
        },
        font_style: FontStyle::empty(),
    }
}
