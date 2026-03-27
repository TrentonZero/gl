use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

static LOGGER_STATE: Mutex<LoggerState> = Mutex::new(LoggerState::new());

pub fn init() {
    let mut state = LOGGER_STATE.lock().unwrap();
    ensure_initialized(&mut state);
}

pub fn profiling_enabled() -> bool {
    let mut state = LOGGER_STATE.lock().unwrap();
    ensure_initialized(&mut state);
    state.profiling_enabled
}

pub fn info(message: impl AsRef<str>) {
    write_record("INFO", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    write_record("WARN", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    write_record("ERROR", message.as_ref());
}

pub fn profile(message: impl AsRef<str>) {
    write_record("PROFILE", message.as_ref());
}

fn write_record(level: &str, message: &str) {
    let mut state = LOGGER_STATE.lock().unwrap();
    ensure_initialized(&mut state);

    let timestamp = unix_timestamp_ms();
    let line = format!("[gl][{level}][{timestamp}] {message}\n");
    let mut wrote_file = false;

    if let Some(file) = state.file.as_mut() {
        if file.write_all(line.as_bytes()).is_ok() && file.flush().is_ok() {
            wrote_file = true;
        }
    }

    if state.mirror_stderr || !wrote_file {
        let _ = io::stderr().write_all(line.as_bytes());
    }
}

fn ensure_initialized(state: &mut LoggerState) {
    if state.initialized {
        return;
    }

    state.initialized = true;
    state.profiling_enabled = env_flag("GL_PROFILE");
    state.mirror_stderr = env_flag("GL_LOG_STDERR");
    state.path = resolve_log_path();

    match open_log_file(&state.path) {
        Ok(file) => {
            state.file = Some(file);
            let line = format!(
                "[gl][INFO][{}] logger initialized path={} profile={} stderr={}\n",
                unix_timestamp_ms(),
                state.path.display(),
                state.profiling_enabled,
                state.mirror_stderr
            );
            if let Some(file) = state.file.as_mut() {
                let _ = file.write_all(line.as_bytes());
                let _ = file.flush();
            }
        }
        Err(error) => {
            let line = format!(
                "[gl][WARN][{}] failed to open log file {}: {}\n",
                unix_timestamp_ms(),
                state.path.display(),
                error
            );
            let _ = io::stderr().write_all(line.as_bytes());
        }
    }
}

fn open_log_file(path: &PathBuf) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    OpenOptions::new().create(true).append(true).open(path)
}

fn resolve_log_path() -> PathBuf {
    resolve_log_path_from_values(
        env::var_os("GL_LOG_PATH")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    )
}

fn resolve_log_path_from_values(
    explicit_path: Option<PathBuf>,
    xdg_state_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> PathBuf {
    if let Some(path) = explicit_path {
        return path;
    }
    if let Some(path) = xdg_state_home {
        return path.join("gl").join("gl.log");
    }
    if let Some(path) = home {
        return path.join(".local").join("state").join("gl").join("gl.log");
    }
    env::temp_dir().join("gl.log")
}

fn env_flag(name: &str) -> bool {
    env_flag_value(env::var(name).ok().as_deref())
}

fn env_flag_value(value: Option<&str>) -> bool {
    match value {
        Some(value) => !matches!(value, "" | "0" | "false" | "FALSE" | "False"),
        None => false,
    }
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

struct LoggerState {
    initialized: bool,
    profiling_enabled: bool,
    mirror_stderr: bool,
    path: PathBuf,
    file: Option<File>,
}

impl LoggerState {
    const fn new() -> Self {
        Self {
            initialized: false,
            profiling_enabled: false,
            mirror_stderr: false,
            path: PathBuf::new(),
            file: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn env_flag_treats_zero_and_false_as_disabled() {
        assert!(env_flag_value(Some("1")));
        assert!(env_flag_value(Some("yes")));
        assert!(!env_flag_value(Some("0")));
        assert!(!env_flag_value(Some("false")));
        assert!(!env_flag_value(Some("")));
        assert!(!env_flag_value(None));
    }

    #[test]
    fn resolve_log_path_prefers_explicit_path() {
        let path = resolve_log_path_from_values(
            Some(PathBuf::from("/tmp/custom.log")),
            Some(PathBuf::from("/xdg")),
            Some(PathBuf::from("/home/test")),
        );

        assert_eq!(path, PathBuf::from("/tmp/custom.log"));
    }

    #[test]
    fn resolve_log_path_uses_xdg_state_home_before_home() {
        let path = resolve_log_path_from_values(
            None,
            Some(PathBuf::from("/xdg")),
            Some(PathBuf::from("/home/test")),
        );

        assert_eq!(path, PathBuf::from("/xdg/gl/gl.log"));
    }

    #[test]
    fn open_log_file_creates_missing_parent_directories() {
        let log_dir = env::temp_dir().join(format!("gl-logger-test-{}", unix_timestamp_ms()));
        let log_path = log_dir.join("gl.log");

        let _file = open_log_file(&log_path).unwrap();

        assert!(log_path.exists());

        let _ = fs::remove_file(&log_path);
        let _ = fs::remove_dir_all(&log_dir);
    }
}
