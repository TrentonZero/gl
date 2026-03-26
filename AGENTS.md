# AGENTS

## Working Norms

- Every body of work should include classical-style, behavior-based tests whenever the change can be covered that way.
- Prefer tests that prove user-visible behavior and regressions over tests that only exercise implementation details.
- Startup should not visibly jump after first paint. Anything required to draw the UI in its basic shape must be loaded before first paint; only decorative stack metadata may be loaded lazily afterward.
- If `cargo build` reports warnings, fix the warnings as part of the work.
- As work progresses, update the project plan to reflect what has been completed.
