# GL — Implementation Plan

**Date:** March 27, 2026

This plan replaces the previous feature-delivery plan with a cleanup-focused plan. The goal is to remove misleading surface area, align visible behavior with configuration, and make the codebase easier to evolve without changing the product's core scope.

Each phase should end in a shippable, tested state.

---

## Goals

- Make all user-facing help and prompts match the actual configured keybindings.
- Remove or implement dead and misleading configuration surface area.
- Align config path resolution with existing XDG-aware cache and log behavior.
- Reduce maintenance risk by extracting large responsibilities out of `src/main.rs`.
- Keep `cargo build` and `cargo test` clean after every phase.

---

## Phase 1: Dynamic Help And Keybinding Consistency

**Status:** Complete

Completed:

- Bottom help bar text now uses the active configured keybindings instead of hardcoded defaults.
- Help overlay text now uses the same keybinding source and includes configured graph and command shortcuts.
- UI tests now verify remapped keys appear in both footer help and overlay help.

**Goal:** Ensure all user-visible help text reflects the active keybinding configuration.

**Tasks:**

1. Replace hardcoded key labels in the bottom help bar with values derived from `AppConfig.keybindings`.
2. Replace hardcoded key labels in the help overlay with the same shared keybinding-driven rendering.
3. Centralize help-text generation so branch list, detail view, stack view, and status view do not drift separately.
4. Add behavior-based tests proving that remapped keys appear correctly in visible help output.
5. Confirm default help output remains unchanged for the default config.

**Deliverable:** A user who remaps keys sees correct instructions everywhere in the UI.

---

## Phase 2: Remove Or Implement Dead Config Surface

**Status:** Complete

Completed:

- Removed the unused `worktree_path_defaults` config field from runtime config parsing.
- Removed the startup no-op read that kept the dead setting alive.
- Updated tests and README config examples so the documented config surface matches the implemented one.

**Goal:** Eliminate misleading configuration that is documented but not functional.

**Tasks:**

1. Decide whether `worktree_path_defaults` should be implemented now or removed from the product surface.
2. If removing it:
   - delete the field from config loading
   - remove the no-op read in app startup
   - remove documentation and tests that describe it as supported
3. If implementing it:
   - define the exact user-visible behavior
   - wire it into the worktree flow
   - add behavior-based coverage proving the feature works from the user's perspective
4. Remove any remaining dead-code allowances that only existed to support abandoned or unused paths.
5. Update README and help text so supported options match reality exactly.

**Deliverable:** No config option is documented or accepted unless it has real runtime behavior.

---

## Phase 3: XDG-Compliant Config Resolution

**Status:** Complete

Completed:

- Config loading now prefers `$XDG_CONFIG_HOME/gl/config.toml` and falls back to `~/.config/gl/config.toml`.
- Added focused tests for XDG precedence, HOME fallback, and missing-environment behavior.
- Updated README config-path documentation to describe the actual lookup order.

**Goal:** Make config discovery consistent with the rest of the app's filesystem conventions.

**Tasks:**

1. Update config loading to prefer `$XDG_CONFIG_HOME/gl/config.toml` when available.
2. Preserve `~/.config/gl/config.toml` as the fallback when `XDG_CONFIG_HOME` is unset.
3. Add focused tests covering:
   - explicit XDG config resolution
   - HOME-based fallback resolution
   - default config behavior when no config file exists
4. Review README config-path documentation and update it to describe the actual precedence order.

**Deliverable:** Config loads from the expected XDG path order, and the docs match the implementation.

---

## Phase 4: Structural Refactor Of App Runtime

**Status:** Complete

Completed:

- Moved CLI parsing and help text into `src/cli.rs`.
- Moved stack/display view-model helpers into `src/view_state.rs` with local tests.
- Reduced `src/main.rs` by moving cohesive responsibilities into dedicated modules while preserving current behavior.

**Goal:** Reduce maintenance risk by splitting large, mixed-responsibility runtime code into smaller modules.

**Tasks:**

1. Break `src/main.rs` into focused modules for:
   - CLI parsing
   - app state
   - event handling and key dispatch
   - background loading and refresh orchestration
   - view-model construction helpers
2. Keep the top-level startup path minimal and readable.
3. Move tests alongside the modules they validate where that improves locality without weakening behavior coverage.
4. Preserve current first-paint behavior so startup shape does not regress.
5. Run the full test suite after each extraction step to catch behavior drift early.

**Deliverable:** The runtime is organized by responsibility rather than accumulated feature history, with no behavior regressions.

---

## Phase 5: Final Cleanup Pass

**Status:** Complete

Completed:

- Removed residual dead-code allowances in `src/stack.rs` that were no longer needed.
- Revalidated the codebase with clean `cargo build` and `cargo test` runs after the refactor.
- Reduced the `src/ui.rs` help-bar renderer input surface to eliminate the remaining `clippy::too_many_arguments` warning.
- Updated this plan to reflect the completed cleanup phases.
- Moved worktree discovery and per-worktree dirty-state loading off the startup critical path so the initial branch list can render before worktree metadata backfills.

**Goal:** Close out residual cleanup debt introduced or exposed by earlier phases.

**Tasks:**

1. Remove stale comments, unused helpers, and obsolete compatibility shims left behind by the refactor.
2. Revisit module interfaces and tighten visibility where possible.
3. Verify there are no compiler warnings, dead-code suppressions without justification, or outdated docs.
4. Run `cargo build` and `cargo test` as final acceptance checks.
5. Update this plan with completed status notes once all phases land.

**Deliverable:** A smaller, more coherent codebase with accurate docs and clean build/test output.
