use crate::config::ColorScheme;
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CliArgs {
    pub(crate) repo_path: Option<PathBuf>,
    pub(crate) show_help: bool,
    pub(crate) show_version: bool,
    pub(crate) color_scheme: Option<ColorScheme>,
}

pub(crate) fn parse_cli_args(args: Vec<String>) -> Result<CliArgs> {
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

pub(crate) fn print_help() {
    println!("{}", help_text());
}

pub(crate) fn help_text() -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
