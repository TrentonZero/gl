use std::{
    env,
    sync::OnceLock,
    time::{Duration, Instant},
};

static ENABLED: OnceLock<bool> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| match env::var("GL_PROFILE") {
        Ok(value) => value != "0" && !value.is_empty(),
        Err(_) => false,
    })
}

pub fn log(label: impl AsRef<str>) {
    if !enabled() {
        return;
    }

    let elapsed = START.get_or_init(Instant::now).elapsed();
    eprintln!(
        "[gl-profile +{:>6}ms] {}",
        elapsed.as_millis(),
        label.as_ref()
    );
}

pub struct ScopeTimer {
    label: String,
    start: Instant,
}

impl ScopeTimer {
    pub fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        if enabled() {
            log(format!("{label} start"));
        }

        Self {
            label,
            start: Instant::now(),
        }
    }
}

impl Drop for ScopeTimer {
    fn drop(&mut self) {
        if !enabled() {
            return;
        }

        log(format!(
            "{} done in {}",
            self.label,
            format_duration(self.start.elapsed())
        ));
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() > 0 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{}us", duration.as_micros())
    }
}
