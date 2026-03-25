import { useState } from "react";

const COLORS = {
  bg: "#1a1b26",
  bgAlt: "#1e2030",
  bgHighlight: "#292e42",
  bgSelected: "#33467c",
  border: "#3b4261",
  text: "#a9b1d6",
  textDim: "#565f89",
  textBright: "#c0caf5",
  green: "#9ece6a",
  greenDim: "#5a7a3a",
  red: "#f7768e",
  redDim: "#9a4a5a",
  yellow: "#e0af68",
  blue: "#7aa2f7",
  purple: "#bb9af7",
  cyan: "#7dcfff",
  orange: "#ff9e64",
  white: "#c0caf5",
};

const mono = "'JetBrains Mono', 'Fira Code', 'SF Mono', 'Cascadia Code', Consolas, monospace";

function StatusBar({ view }) {
  const viewLabels = {
    branches: "1:Branches",
    detail: "2:Stack",
    worktrees: "3:Worktrees",
    graph: "4:Graph",
    status: "5:Status",
  };
  return (
    <div style={{
      display: "flex", justifyContent: "space-between",
      background: COLORS.blue, color: COLORS.bg,
      fontFamily: mono, fontSize: 13, fontWeight: 700,
      padding: "2px 8px", letterSpacing: 0.3,
    }}>
      <span>GL — Green Ledger</span>
      <span style={{ display: "flex", gap: 16 }}>
        {Object.entries(viewLabels).map(([k, v]) => (
          <span key={k} style={{
            opacity: view === k ? 1 : 0.5,
            textDecoration: view === k ? "underline" : "none",
            textUnderlineOffset: 2,
          }}>{v}</span>
        ))}
      </span>
      <span>~/code/myproject</span>
    </div>
  );
}

function HelpBar({ hints }) {
  return (
    <div style={{
      display: "flex", gap: 16, background: COLORS.bgAlt,
      borderTop: `1px solid ${COLORS.border}`,
      fontFamily: mono, fontSize: 12, padding: "3px 8px", color: COLORS.textDim,
    }}>
      {hints.map((h, i) => (
        <span key={i}>
          <span style={{ color: COLORS.yellow }}>{h[0]}</span>
          <span> {h[1]}</span>
        </span>
      ))}
    </div>
  );
}

function BranchListView({ onSelect }) {
  const [selected, setSelected] = useState(2);
  const branches = [
    { type: "header", label: "feature/auth stack" },
    { type: "stack", name: "feature/auth-base", commits: 3, ahead: 0, behind: 0, worktree: null, stale: false, indent: 1 },
    { type: "stack", name: "feature/auth-middleware", commits: 2, ahead: 1, behind: 0, worktree: "wt-2", stale: false, indent: 2 },
    { type: "stack", name: "feature/auth-ui", commits: 4, ahead: 4, behind: 0, worktree: null, stale: true, indent: 3 },
    { type: "header", label: "feature/payments stack" },
    { type: "stack", name: "feature/payments-model", commits: 1, ahead: 0, behind: 2, worktree: null, stale: false, indent: 1 },
    { type: "stack", name: "feature/payments-api", commits: 5, ahead: 5, behind: 0, worktree: null, stale: false, indent: 2 },
    { type: "header", label: "standalone" },
    { type: "branch", name: "fix/typo-readme", commits: 1, ahead: 0, behind: 0, worktree: null, stale: false },
    { type: "branch", name: "chore/deps-update", commits: 1, ahead: 1, behind: 0, worktree: null, stale: false },
    { type: "branch", name: "main", commits: 0, ahead: 0, behind: 0, worktree: "main", stale: false, isCurrent: true },
  ];

  const selectableIndices = branches.map((b, i) => b.type !== "header" ? i : null).filter(i => i !== null);

  return (
    <div style={{ flex: 1, overflow: "auto" }}>
      {branches.map((b, i) => {
        if (b.type === "header") {
          return (
            <div key={i} style={{
              fontFamily: mono, fontSize: 12, color: COLORS.purple,
              padding: "6px 8px 2px", fontWeight: 700,
              borderBottom: `1px solid ${COLORS.border}`,
              marginTop: i > 0 ? 4 : 0,
            }}>
              {b.label}
            </div>
          );
        }
        const isSelected = i === selectableIndices[selected];
        const indent = b.indent || 0;
        return (
          <div
            key={i}
            onClick={() => {
              const idx = selectableIndices.indexOf(i);
              if (idx >= 0) { setSelected(idx); onSelect(b); }
            }}
            style={{
              fontFamily: mono, fontSize: 13,
              padding: "3px 8px 3px " + (8 + indent * 16) + "px",
              background: isSelected ? COLORS.bgSelected : "transparent",
              color: isSelected ? COLORS.textBright : COLORS.text,
              cursor: "pointer",
              display: "flex", justifyContent: "space-between", alignItems: "center",
              borderLeft: indent > 0 ? `1px solid ${COLORS.border}` : "none",
              marginLeft: indent > 0 ? 8 + (indent - 1) * 16 : 0,
            }}
          >
            <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
              {b.stale && <span style={{ color: COLORS.yellow }} title="needs rebase">⚠</span>}
              {b.isCurrent && <span style={{ color: COLORS.green }}>●</span>}
              <span>{b.name}</span>
              {b.worktree && (
                <span style={{
                  color: COLORS.cyan, fontSize: 11,
                  border: `1px solid ${COLORS.cyan}33`,
                  borderRadius: 3, padding: "0 4px",
                }}>{b.worktree}</span>
              )}
            </span>
            <span style={{ display: "flex", gap: 10, fontSize: 12 }}>
              {b.commits > 0 && <span style={{ color: COLORS.textDim }}>{b.commits}c</span>}
              {b.ahead > 0 && <span style={{ color: COLORS.green }}>↑{b.ahead}</span>}
              {b.behind > 0 && <span style={{ color: COLORS.red }}>↓{b.behind}</span>}
              {b.ahead === 0 && b.behind === 0 && b.commits > 0 && <span style={{ color: COLORS.textDim }}>✓</span>}
            </span>
          </div>
        );
      })}
    </div>
  );
}

function BranchDetailView() {
  const diffLines = [
    { type: "file", text: "── src/auth/middleware.rs ──────────────────────────── +42 -8" },
    { type: "hunk", text: "@@ -1,12 +1,46 @@" },
    { type: "ctx",  text: " use axum::{middleware::Next, response::Response};" },
    { type: "ctx",  text: " use axum::extract::Request;" },
    { type: "add",  text: "+use jsonwebtoken::{decode, DecodingKey, Validation};" },
    { type: "add",  text: "+use crate::auth::claims::Claims;" },
    { type: "ctx",  text: "" },
    { type: "del",  text: "-pub async fn auth_middleware(req: Request, next: Next) -> Response {" },
    { type: "del",  text: "-    // TODO: implement auth" },
    { type: "del",  text: "-    next.run(req).await" },
    { type: "add",  text: "+pub async fn auth_middleware(" },
    { type: "add",  text: "+    req: Request," },
    { type: "add",  text: "+    next: Next," },
    { type: "add",  text: "+) -> Result<Response, AuthError> {" },
    { type: "add",  text: "+    let token = req" },
    { type: "add",  text: '+        .headers()' },
    { type: "add",  text: '+        .get("Authorization")' },
    { type: "add",  text: "+        .and_then(|v| v.to_str().ok())" },
    { type: "add",  text: '+        .and_then(|v| v.strip_prefix("Bearer "));' },
    { type: "ctx",  text: "" },
    { type: "add",  text: "+    let token = match token {" },
    { type: "add",  text: "+        Some(t) => t," },
    { type: "add",  text: "+        None => return Err(AuthError::MissingToken)," },
    { type: "add",  text: "+    };" },
    { type: "ctx",  text: "" },
    { type: "add",  text: "+    let claims = decode::<Claims>(" },
    { type: "add",  text: "+        token," },
    { type: "add",  text: "+        &DecodingKey::from_secret(SECRET.as_ref())," },
    { type: "add",  text: "+        &Validation::default()," },
    { type: "add",  text: "+    )" },
    { type: "add",  text: "+    .map_err(|_| AuthError::InvalidToken)?;" },
    { type: "ctx",  text: "" },
    { type: "add",  text: "+    req.extensions_mut().insert(claims.claims);" },
    { type: "add",  text: "+    Ok(next.run(req).await)" },
    { type: "ctx",  text: " }" },
    { type: "file", text: "── src/auth/mod.rs ────────────────────────────────── +3 -1" },
    { type: "hunk", text: "@@ -1,4 +1,6 @@" },
    { type: "ctx",  text: " pub mod handlers;" },
    { type: "add",  text: "+pub mod claims;" },
    { type: "add",  text: "+pub mod middleware;" },
    { type: "ctx",  text: "" },
    { type: "del",  text: "-pub use handlers::*;" },
    { type: "add",  text: "+pub use handlers::auth_handler;" },
    { type: "ctx",  text: " pub use handlers::health_check;" },
    { type: "file", text: "── tests/auth_test.rs ─────────────────────────────── +28 -0" },
    { type: "hunk", text: "@@ -0,0 +1,28 @@" },
    { type: "add",  text: "+use myproject::auth::middleware::auth_middleware;" },
    { type: "add",  text: "+use axum::test_helpers::*;" },
    { type: "add",  text: "+" },
    { type: "add",  text: "+#[tokio::test]" },
    { type: "add",  text: "+async fn test_missing_token_returns_401() {" },
    { type: "add",  text: "+    let app = test_app().layer(axum::middleware::from_fn(auth_middleware));" },
    { type: "add",  text: '+    let res = app.oneshot(Request::get("/protected").body(Body::empty()).unwrap()).await;' },
    { type: "add",  text: "+    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);" },
    { type: "add",  text: "+}" },
    { type: "add",  text: "+" },
    { type: "add",  text: "+#[tokio::test]" },
    { type: "add",  text: "+async fn test_valid_token_passes_through() {" },
    { type: "add",  text: "+    let token = create_test_token();" },
    { type: "add",  text: "+    let app = test_app().layer(axum::middleware::from_fn(auth_middleware));" },
    { type: "add",  text: "+    let res = app.oneshot(" },
    { type: "add",  text: '+        Request::get("/protected")' },
    { type: "add",  text: '+            .header("Authorization", format!("Bearer {token}"))' },
    { type: "add",  text: "+            .body(Body::empty())" },
    { type: "add",  text: "+            .unwrap()" },
    { type: "add",  text: "+    ).await;" },
    { type: "add",  text: "+    assert_eq!(res.status(), StatusCode::OK);" },
    { type: "add",  text: "+}" },
  ];

  const lineColors = {
    file: COLORS.purple,
    hunk: COLORS.cyan,
    ctx: COLORS.textDim,
    add: COLORS.green,
    del: COLORS.red,
  };
  const lineBg = {
    add: COLORS.greenDim + "18",
    del: COLORS.redDim + "18",
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: 1, overflow: "hidden" }}>
      {/* diff — entire pane */}
      <div style={{ flex: 1, overflow: "auto", padding: "4px 0" }}>
        {diffLines.map((line, i) => (
          <div key={i} style={{
            fontFamily: mono, fontSize: 13, lineHeight: "20px",
            padding: "0 12px",
            color: lineColors[line.type] || COLORS.text,
            background: lineBg[line.type] || "transparent",
            fontWeight: line.type === "file" ? 700 : 400,
            borderTop: line.type === "file" && i > 0 ? `1px solid ${COLORS.border}` : "none",
            marginTop: line.type === "file" && i > 0 ? 8 : 0,
            paddingTop: line.type === "file" ? 6 : 0,
          }}>
            <span style={{ display: "inline-block", width: 36, textAlign: "right", marginRight: 12, color: COLORS.textDim, fontSize: 11, opacity: line.type === "file" || line.type === "hunk" ? 0 : 0.6 }}>
              {line.type !== "file" && line.type !== "hunk" ? i : ""}
            </span>
            {line.text || " "}
          </div>
        ))}
      </div>
    </div>
  );
}

function WorktreeManagerView() {
  const [selected, setSelected] = useState(0);
  const worktrees = [
    { path: "~/code/myproject", branch: "main", status: "clean", isBare: false, isCurrent: true },
    { path: "~/code/myproject-wt-2", branch: "feature/auth-middleware", status: "dirty (2 files)", isBare: false, isCurrent: false },
  ];
  return (
    <div style={{ flex: 1, padding: 12 }}>
      <div style={{ fontFamily: mono, fontSize: 12, color: COLORS.textDim, marginBottom: 8 }}>
        bare repo: ~/code/myproject/.git
      </div>
      {worktrees.map((wt, i) => (
        <div
          key={i}
          onClick={() => setSelected(i)}
          style={{
            fontFamily: mono, fontSize: 13,
            padding: "8px 12px", marginBottom: 4, borderRadius: 4,
            background: selected === i ? COLORS.bgSelected : COLORS.bgAlt,
            border: `1px solid ${selected === i ? COLORS.blue : COLORS.border}`,
            cursor: "pointer",
          }}
        >
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <span style={{ color: COLORS.textBright, fontWeight: 700 }}>
              {wt.isCurrent && <span style={{ color: COLORS.green }}>● </span>}
              {wt.path}
            </span>
            <span style={{
              color: wt.status === "clean" ? COLORS.green : COLORS.yellow,
              fontSize: 12,
            }}>{wt.status}</span>
          </div>
          <div style={{ color: COLORS.blue, fontSize: 12, marginTop: 2 }}>
            → {wt.branch}
          </div>
        </div>
      ))}
    </div>
  );
}

function StackView() {
  const [selected, setSelected] = useState(2);
  const branches = [
    { name: "feature/auth-ui", commits: 4, files: 8, adds: 156, dels: 23, pushed: false, stale: true },
    { name: "feature/auth-middleware", commits: 2, files: 3, adds: 73, dels: 9, pushed: false, stale: false },
    { name: "feature/auth-base", commits: 3, files: 5, adds: 112, dels: 34, pushed: true, stale: false },
  ];
  // Render tip at top, base at bottom
  return (
    <div style={{ flex: 1, padding: 12, display: "flex", flexDirection: "column" }}>
      <div style={{
        fontFamily: mono, fontSize: 12, color: COLORS.purple, fontWeight: 700,
        marginBottom: 8, paddingBottom: 6, borderBottom: `1px solid ${COLORS.border}`,
      }}>
        feature/auth stack — 3 branches, 9 commits
      </div>
      <div style={{ flex: 1 }}>
        {branches.map((b, i) => {
          const isSelected = i === (branches.length - 1 - selected);
          const isTop = i === 0;
          const isBottom = i === branches.length - 1;
          return (
            <div key={i} style={{ display: "flex", marginBottom: 0 }}>
              {/* tree line */}
              <div style={{
                width: 24, display: "flex", flexDirection: "column", alignItems: "center",
                fontFamily: mono, fontSize: 13, color: COLORS.border,
              }}>
                {!isTop && <div style={{ width: 1, height: 8, background: COLORS.border }} />}
                <div style={{
                  width: 10, height: 10, borderRadius: "50%",
                  background: b.stale ? COLORS.yellow : b.pushed ? COLORS.green : COLORS.textDim,
                  border: isSelected ? `2px solid ${COLORS.blue}` : `2px solid ${COLORS.border}`,
                  flexShrink: 0,
                }} />
                {!isBottom && <div style={{ width: 1, flex: 1, background: COLORS.border }} />}
              </div>
              {/* branch card */}
              <div
                onClick={() => setSelected(branches.length - 1 - i)}
                style={{
                  flex: 1, fontFamily: mono, fontSize: 13,
                  padding: "6px 10px", marginBottom: 2, borderRadius: 3,
                  background: isSelected ? COLORS.bgSelected : COLORS.bgAlt,
                  border: `1px solid ${isSelected ? COLORS.blue : COLORS.border}`,
                  cursor: "pointer",
                }}
              >
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    {b.stale && <span style={{ color: COLORS.yellow }}>⚠</span>}
                    <span style={{ color: COLORS.textBright, fontWeight: 700 }}>{b.name}</span>
                  </span>
                  <span style={{ fontSize: 12 }}>
                    {b.pushed
                      ? <span style={{ color: COLORS.green }}>✓ pushed</span>
                      : <span style={{ color: COLORS.orange }}>unpushed</span>
                    }
                  </span>
                </div>
                <div style={{ fontSize: 12, color: COLORS.textDim, marginTop: 2, display: "flex", gap: 12 }}>
                  <span>{b.commits} commits</span>
                  <span>{b.files} files</span>
                  <span style={{ color: COLORS.green }}>+{b.adds}</span>
                  <span style={{ color: COLORS.red }}>-{b.dels}</span>
                </div>
              </div>
            </div>
          );
        })}
      </div>
      <div style={{
        fontFamily: mono, fontSize: 12, color: COLORS.textDim,
        marginTop: 8, paddingTop: 6, borderTop: `1px solid ${COLORS.border}`,
      }}>
        base → main
      </div>
    </div>
  );
}

function GraphView() {
  const commits = [
    { hash: "a3f21bc", msg: "add JWT validation to middleware", branch: "feature/auth-middleware", graph: "● " },
    { hash: "e7c904d", msg: "extract auth module from handlers", branch: "feature/auth-middleware", graph: "● " },
    { hash: "f12ab8e", msg: "set up auth base types and error enum", branch: "feature/auth-base", graph: "● " },
    { hash: "c9d3f1a", msg: "add Claims struct and validation helper", branch: "feature/auth-base", graph: "● " },
    { hash: "b8e72cd", msg: "scaffold auth module with feature flag", branch: "feature/auth-base", graph: "● " },
    { hash: "3a91ef2", msg: "Merge PR #142 payments-model", branch: "main", graph: "●─╮" },
    { hash: "7f2c8a1", msg: "fix typo in README contributing section", branch: "fix/typo-readme", graph: "│ ●" },
    { hash: "1b4de9f", msg: "bump deps to latest patch versions", branch: "main", graph: "● │" },
    { hash: "8c5a3b7", msg: "release v2.3.1", branch: "main", graph: "●─╯" },
  ];

  return (
    <div style={{ flex: 1, overflow: "auto", padding: "8px 0" }}>
      <div style={{
        fontFamily: mono, fontSize: 12, color: COLORS.textDim,
        padding: "0 12px 8px", borderBottom: `1px solid ${COLORS.border}`, marginBottom: 4,
      }}>
        first-parent only · local branches
      </div>
      {commits.map((c, i) => (
        <div key={i} style={{
          fontFamily: mono, fontSize: 13, lineHeight: "22px",
          padding: "0 12px", display: "flex", gap: 0,
        }}>
          <span style={{ color: COLORS.green, width: 36, flexShrink: 0 }}>{c.graph}</span>
          <span style={{ color: COLORS.yellow, width: 72, flexShrink: 0 }}>{c.hash}</span>
          <span style={{ color: COLORS.text, flex: 1 }}>{c.msg}</span>
          <span style={{ color: COLORS.blue, fontSize: 11, flexShrink: 0 }}>{c.branch}</span>
        </div>
      ))}
    </div>
  );
}

function StatusView() {
  const [section, setSection] = useState("staged");
  const [expanded, setExpanded] = useState(null);

  const staged = [
    { path: "src/auth/claims.rs", stat: "+34 -0", status: "A",
      diff: [
        { type: "add", text: "+use serde::{Deserialize, Serialize};" },
        { type: "add", text: "+" },
        { type: "add", text: "+#[derive(Debug, Serialize, Deserialize)]" },
        { type: "add", text: "+pub struct Claims {" },
        { type: "add", text: "+    pub sub: String," },
        { type: "add", text: "+    pub exp: usize," },
        { type: "add", text: "+    pub role: String," },
        { type: "add", text: "+}" },
      ]},
    { path: "src/auth/mod.rs", stat: "+1 -0", status: "M",
      diff: [
        { type: "ctx", text: " pub mod handlers;" },
        { type: "add", text: "+pub mod claims;" },
        { type: "ctx", text: " pub mod middleware;" },
      ]},
  ];

  const unstaged = [
    { path: "src/auth/middleware.rs", stat: "+3 -1", status: "M",
      diff: [
        { type: "ctx", text: "     let claims = decode::<Claims>(" },
        { type: "ctx", text: "         token," },
        { type: "del", text: "-        &DecodingKey::from_secret(SECRET.as_ref())," },
        { type: "add", text: "+        &DecodingKey::from_secret(" },
        { type: "add", text: "+            std::env::var(\"JWT_SECRET\")" },
        { type: "add", text: "+                .expect(\"JWT_SECRET must be set\").as_ref()," },
        { type: "ctx", text: "         &Validation::default()," },
      ]},
    { path: "tests/auth_test.rs", stat: "+5 -0", status: "M", diff: [] },
    { path: ".env.example", stat: "", status: "?", diff: [] },
  ];

  const lineColors = { ctx: COLORS.textDim, add: COLORS.green, del: COLORS.red };
  const lineBg = { add: COLORS.greenDim + "18", del: COLORS.redDim + "18" };
  const statusColors = { A: COLORS.green, M: COLORS.yellow, "?": COLORS.textDim };

  const renderSection = (label, files, sectionKey) => {
    const isFocused = section === sectionKey;
    return (
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <div
          onClick={() => setSection(sectionKey)}
          style={{
            fontFamily: mono, fontSize: 12, fontWeight: 700, padding: "4px 12px",
            color: isFocused ? COLORS.textBright : COLORS.textDim,
            background: isFocused ? COLORS.bgHighlight : "transparent",
            borderBottom: `1px solid ${COLORS.border}`, cursor: "pointer",
          }}
        >
          {label} ({files.length} {files.length === 1 ? "file" : "files"})
        </div>
        <div style={{ flex: 1, overflow: "auto" }}>
          {files.map((f, i) => {
            const key = sectionKey + "-" + i;
            const isExpanded = expanded === key;
            return (
              <div key={i}>
                <div
                  onClick={() => setExpanded(isExpanded ? null : key)}
                  style={{
                    fontFamily: mono, fontSize: 13, padding: "3px 12px",
                    display: "flex", justifyContent: "space-between", cursor: "pointer",
                    background: isExpanded ? COLORS.bgSelected : "transparent",
                  }}
                >
                  <span style={{ display: "flex", gap: 8 }}>
                    <span style={{ color: statusColors[f.status] || COLORS.text, width: 16 }}>{f.status}</span>
                    <span style={{ color: COLORS.text }}>{f.path}</span>
                  </span>
                  <span style={{ color: COLORS.textDim, fontSize: 12 }}>{f.stat}</span>
                </div>
                {isExpanded && f.diff.length > 0 && (
                  <div style={{ borderBottom: `1px solid ${COLORS.border}` }}>
                    {f.diff.map((line, li) => (
                      <div key={li} style={{
                        fontFamily: mono, fontSize: 12, lineHeight: "18px",
                        padding: "0 12px 0 40px",
                        color: lineColors[line.type] || COLORS.text,
                        background: lineBg[line.type] || "transparent",
                      }}>
                        {line.text}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    );
  };

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      {renderSection("Staged", staged, "staged")}
      <div style={{ height: 1, background: COLORS.border }} />
      {renderSection("Unstaged", unstaged, "unstaged")}
    </div>
  );
}

export default function App() {
  const [view, setView] = useState("branches");
  const [selectedBranch, setSelectedBranch] = useState(null);

  const helpHints = {
    branches: [["j/k", "navigate"], ["Enter", "open branch"], ["s", "stack view"], ["w", "worktrees"], ["/", "filter"], ["q", "quit"]],
    detail: [["j/k", "scroll"], ["J/K", "next/prev file"], ["Tab", "commits"], ["v", "side-by-side"], ["[/]", "prev/next branch"], ["q", "quit"]],
    worktrees: [["j/k", "navigate"], ["Enter", "switch context"], ["!", "open terminal"], ["q", "quit"]],
    graph: [["j/k", "navigate"], ["J/K", "next/prev branch"], ["Enter", "open branch"], ["e", "expand merge"], ["q", "quit"]],
    status: [["j/k", "navigate"], ["J/K", "next/prev file"], ["Tab", "staged/unstaged"], ["Enter", "expand diff"], ["q", "quit"]],
  };

  const showDetail = view === "branches" && selectedBranch;

  return (
    <div style={{
      width: "100%", maxWidth: 1000, margin: "0 auto",
      height: "100vh", maxHeight: 700,
      display: "flex", flexDirection: "column",
      background: COLORS.bg, color: COLORS.text,
      border: `1px solid ${COLORS.border}`,
      borderRadius: 6, overflow: "hidden",
      boxShadow: "0 8px 32px rgba(0,0,0,0.5)",
    }}>
      <StatusBar view={view === "branches" ? (showDetail ? "branches" : "branches") : view} />

      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        {view === "branches" && !showDetail && (
          <BranchListView onSelect={(b) => { if (b.type !== "header") setSelectedBranch(b); }} />
        )}
        {view === "branches" && showDetail && (
          <>
            <div style={{
              width: 240, borderRight: `1px solid ${COLORS.border}`,
              overflow: "auto", flexShrink: 0,
            }}>
              <BranchListView onSelect={(b) => { if (b.type !== "header") setSelectedBranch(b); }} />
            </div>
            <BranchDetailView />
          </>
        )}
        {view === "stack" && <StackView />}
        {view === "worktrees" && <WorktreeManagerView />}
        {view === "graph" && <GraphView />}
        {view === "status" && <StatusView />}
      </div>

      <HelpBar hints={helpHints[showDetail ? "detail" : view] || helpHints.branches} />

      {/* view switcher overlay */}
      <div style={{
        position: "absolute", bottom: 32, left: "50%", transform: "translateX(-50%)",
        display: "flex", gap: 4, background: COLORS.bgAlt,
        border: `1px solid ${COLORS.border}`, borderRadius: 6,
        padding: 4, zIndex: 10,
      }}>
        {[
          ["branches", "Branches"],
          ["stack", "Stack"],
          ["worktrees", "Worktrees"],
          ["graph", "Graph"],
          ["status", "Status"],
        ].map(([k, label]) => (
          <button
            key={k}
            onClick={() => { setView(k); if (k !== "branches") setSelectedBranch(null); }}
            style={{
              fontFamily: mono, fontSize: 12,
              padding: "4px 12px", border: "none", borderRadius: 4,
              background: view === k ? COLORS.bgSelected : "transparent",
              color: view === k ? COLORS.textBright : COLORS.textDim,
              cursor: "pointer", fontWeight: view === k ? 700 : 400,
            }}
          >
            {label}
          </button>
        ))}
      </div>
    </div>
  );
}
