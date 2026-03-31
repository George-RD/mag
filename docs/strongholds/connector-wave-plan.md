# Connector Wave Plan

> Stronghold document. Cross-tool connector implementation spec for MAG.
> Status: REVISED — DG R1 incorporated
> Date: 2026-03-31
> Parent: `docs/strongholds/mag-improvement-plan.md` (Ongoing: Competitive Maintenance, Cross-tool)
> Tracks: GitHub issue #166

---

## Problem

Claude Code is the only tool with skills, hooks, and plugin integration. The other 8 supported tools get raw MCP access with no guidance. Some tools accept AGENTS.md files (Codex, GeminiCli) or SKILL.md files (OpenCode) — `mag setup` only writes MCP config today.

The fix is asset creation + installer wiring. No new Rust modules. New functions in `setup.rs`, plus static content files.

---

## Design

### ContentTier enum

Add a `content_tier()` method to `AiTool` in `tool_detection.rs`, following the `config_format()` pattern already there.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentTier {
    /// MCP config only (Claude Desktop, VS Code Copilot, Cline, Zed)
    Mcp,
    /// MCP + AGENTS.md section (Codex, GeminiCli)
    AgentsMd,
    /// MCP + SKILL.md files (OpenCode — directory confirmed)
    Skills,
    /// MCP + project-scoped rules file (Cursor .mdc, Windsurf .md) — Wave B
    Rules,
    /// Full plugin integration (Claude Code) — handled by plugin system
    Plugin,
}

impl AiTool {
    pub fn content_tier(&self) -> ContentTier {
        match self {
            Self::ClaudeCode => ContentTier::Plugin,
            Self::Codex | Self::GeminiCli => ContentTier::AgentsMd,
            Self::OpenCode => ContentTier::Skills,
            Self::Cursor | Self::Windsurf => ContentTier::Rules,
            _ => ContentTier::Mcp,  // ClaudeDesktop, VSCodeCopilot, Cline, Zed
        }
    }
}
```

**Tier notes:**
- `AgentsMd`: Codex reads `~/.codex/AGENTS.md` (global) and `AGENTS.md` at repo/CWD. GeminiCli reads `~/.gemini/AGENTS.md` and `.gemini/AGENTS.md`. Neither has a skills directory or hooks.json. `install_agents_md()` writes/appends a MAG section to the global path for each tool.
- `Skills`: OpenCode skill directory `~/.config/opencode/skills/<name>/SKILL.md` is confirmed by OpenCode docs. Project-scoped `.opencode/skills/<name>/SKILL.md` also exists.
- `Rules`: Project-scoped only. Cursor: `{project_root}/.cursor/rules/mag-memory.mdc`. Windsurf: `{project_root}/.windsurf/rules/mag-memory.md`. Neither tool has a global rules directory that MAG can safely write to (Windsurf's `~/.codeium/windsurf/memories/global_rules.md` is a single overwritable file — writing there would clobber user content). `install_rules()` skips both tools if `project_root` is `None`.

### Asset directory

```
connectors/
  codex/AGENTS.md                   (condensed MAG section: store, recall, checkpoint, health)
  gemini/AGENTS.md                  (same content, GeminiCli path variant)
  opencode/skills/{memory-store,memory-recall,memory-checkpoint,memory-health}/SKILL.md
  cursor/mag-memory.mdc             (Wave B)
  windsurf/mag-memory.md            (Wave B)
```

Shipped via `include_str!()`.

**Codex/GeminiCli:** Single AGENTS.md per tool. Condenses the four skill descriptions into one file rather than four SKILL.md files (which Codex and GeminiCli never read). Written to `~/.codex/AGENTS.md` and `~/.gemini/AGENTS.md` respectively; appends a fenced MAG section if the file already exists.

**OpenCode SKILL.md frontmatter:** Only use fields OpenCode accepts: `name` (required), `description` (required), `license` (optional), `compatibility` (optional), `metadata` (optional). Do NOT include `user-invocable` or `allowed-tools` — these are Claude Code-specific fields that OpenCode does not recognize. Permissions are governed separately via `permission.skill` patterns in OpenCode.

**CLI commands in skill bodies:** All skill files must use working CLI commands: `mag process` for store operations, `mag welcome` for session-start. Do NOT copy `mag hook store` or `mag hook search` from the existing Claude Code source skills — `mag hook` does not exist as a CLI command (parent plan §2C, H1). See F7/dependency note below.

### New functions in setup.rs

```rust
pub fn install_agents_md(tool: AiTool, home: &Path) -> Result<bool>
pub fn install_skills(tool: AiTool, home: &Path) -> Result<usize>
pub fn install_rules(tool: AiTool, home: &Path, project_root: Option<&Path>) -> Result<bool>
```

`install_hooks()` is excluded from Wave A — no confirmed hook targets exist outside Claude Code. Add only when a specific tool confirms lifecycle hook support.

All three functions called from `configure_tools()` after MCP config is written, gated on `tool.content_tier()`. Uninstall path: extend `run_uninstall()` to remove known AGENTS.md sections, skill dirs, and rules files, mirroring the install logic.

---

## Implementation Tasks

### Wave A: Codex + GeminiCli AGENTS.md; OpenCode SKILL.md

**Prerequisite:** Wave A is gated on parent plan Wave 1 skill rewrites (item 6 — fixing `mag hook` calls to use current CLI commands). A2 asset files must be written using corrected CLI commands, not copied from the current broken source skills.

| # | Task | Effort |
|---|------|--------|
| A1 | Add `ContentTier` enum (`AgentsMd` tier included) and `content_tier()` method to `tool_detection.rs` | 1 hr |
| A2 | Create `connectors/codex/AGENTS.md`, `connectors/gemini/AGENTS.md`, and 4 OpenCode SKILL.md files with correct frontmatter and working CLI commands | 2 hr |
| A3 | Implement `install_agents_md()` + `install_skills()`, extend `run_uninstall()`, wire into `configure_tools()` | 4 hr |
| A4 | Tests: positive-path + negative-path cases (see Testing section) | 3 hr |

**Total Wave A: ~1.5 days** (gated on parent Wave 1 completion; add 0.5 day if source skill fixes are not yet done)

Notes:
- A1: ~20 LOC. Follows existing `config_format()` pattern exactly.
- A2: Write fresh from corrected CLI commands, not adapted from broken source. OpenCode SKILL.md is ~20 lines each; AGENTS.md files are one condensed file each.
- A3: ~70 LOC. One match arm per tool. AGENTS.md append logic needs sentinel detection to stay idempotent.

### Wave B: Cursor + Windsurf rules (deferred)

**Install targets (now fully specified):**
- Cursor: `{project_root}/.cursor/rules/mag-memory.mdc`. No global path exists. Skip if `project_root` is `None`.
- Windsurf: `{project_root}/.windsurf/rules/mag-memory.md`. Project-scoped only. Do NOT write to `~/.codeium/windsurf/memories/global_rules.md` — that is a single overwritable file with a 6,000-character limit; writing there would clobber user content.

| # | Task | Effort |
|---|------|--------|
| B1 | Create `connectors/cursor/mag-memory.mdc` and `connectors/windsurf/mag-memory.md` | 1 hr |
| B2 | Implement `install_rules()` in `setup.rs`, extend `run_uninstall()` for rules files, wire into `configure_tools()` | 2 hr |
| B3 | Tests | 1 hr |

**Total Wave B: ~0.5 day**

Deferred until after MCP facade ships (parent plan Wave 2) because rules reference MCP tool names that will change from 16 to 4.

---

## Testing

**Positive-path:**
1. `content_tier()` returns expected value for each `AiTool` variant (including `AgentsMd` for Codex/GeminiCli)
2. `install_skills()` creates expected files in tempdir with correct OpenCode frontmatter (no `user-invocable`, no `allowed-tools`)
3. `install_skills()` is idempotent (second call succeeds, files unchanged)
4. `install_agents_md()` creates AGENTS.md with MAG section; second call is idempotent (sentinel detection)
5. `mag setup` with Codex detected installs both MCP config and AGENTS.md section

**Negative-path:**
6. `install_skills()` returns `Ok(0)` when tool has `ContentTier::Mcp` (no-op — guards against tier routing regression)
7. `install_skills()` returns `Err` if target directory is unwritable
8. `install_skills()` on partial prior install (some files exist, some don't) completes without error

**Manual acceptance:** Invoke a skill in OpenCode session; verify `mag process` executes and stores memory.

---

## Risks

| Risk | Mitigation |
|------|------------|
| Codex/GeminiCli AGENTS.md path changes | `install_agents_md()` is idempotent — re-run `mag setup` to update |
| OpenCode skill directory schema changes | `install_skills()` is idempotent — re-run `mag setup` to update |
| AGENTS.md append clobbers user content | Write within a fenced MAG section with sentinel comments; never replace the whole file |
| Installed skills reference broken CLI commands | Wave A is gated on parent Wave 1 skill fix; asset files written from corrected commands only |
| Wave B rules reference stale MCP tool names | Wave B deferred until MCP facade (Wave 2) ships |
| Stale skills survive `mag uninstall` | `run_uninstall()` extended to remove skill dirs and AGENTS.md sections |

---

## Success Metric

| Metric | Current | Target |
|--------|---------|--------|
| Tools with enriched guidance (beyond raw MCP) | 1 (Claude Code) | 3–4 (+ Codex, GeminiCli, + OpenCode) |

---

## Non-Goals

- Trait hierarchy or BaseConnector — a `match` on `AiTool` is sufficient
- Runtime content generation — all assets are static files
- Auto-capture hooks for non-Claude Code tools — PostToolUse is Claude Code-specific
- `install_hooks()` in Wave A — add only when a specific tool confirms hook support
- Windsurf/Cursor global rules install — project-scoped only

---

## Simplification Log

### Pass 1 (2026-03-31) — initial simplification

- Replaced `ToolProfile` struct (7 fields, `RuleFormat` enum) with `ContentTier` enum + single method
- Collapsed 10 Wave A tasks to 4, 6 Wave B tasks to 3
- Cut effort from 3.5 days → 1.5 days (Wave A), 1.75 days → 0.75 day (Wave B)
- Removed full asset content examples, redundant Problem Statement, Testing items, Risk rows, and "Relationship to Main Plan" section
- Net: ~250 lines → ~130 lines

### Pass 2 (2026-03-31) — ground truth pass

- Removed GeminiCli from `ContentTier::Skills` (no evidence of skill mechanism). Moved to `Mcp` default with note.
- Removed OpenCode from `ContentTier::Skills`. Demoted to `Mcp` with verification note in A2.
- Dropped `install_hooks()` from public interface. Codex hook support was speculative; having it in the signature implied it would ship.
- Flagged broken `mag hook` in source skills. Added explicit caveat pointing to working CLI equivalents.
- Cut Wave A effort 1.5 days → 1 day.
- Collapsed success metrics to one row.
- Net: ~130 lines → ~110 lines.

### Pass 3 (2026-03-31) — DG R1 incorporated (F1–F10)

- **F1 (HIGH):** Codex has no skills directory. Replaced `ContentTier::Skills` for Codex with new `ContentTier::AgentsMd` tier. Asset changes from 4 SKILL.md files to one `connectors/codex/AGENTS.md`. Install target: `~/.codex/AGENTS.md` (append with sentinel).
- **F2 (HIGH):** OpenCode SKILL.md frontmatter corrected. Removed `user-invocable` and `allowed-tools` (Claude Code-specific, not in OpenCode spec). Documented accepted fields: `name`, `description`, `license`, `compatibility`, `metadata`. Fixed `mag hook` → `mag process`/`mag welcome`.
- **F3 (MEDIUM):** Windsurf rules scoped to project only. `~/.codeium/windsurf/memories/global_rules.md` is a single overwritable file; writing there would clobber user content. Install target is `{project_root}/.windsurf/rules/mag-memory.md`. Skip if no project root.
- **F4 (MEDIUM):** Cursor rules install target made explicit: `{project_root}/.cursor/rules/mag-memory.mdc`. No global path exists. Skip if `project_root` is `None`.
- **F5 (MEDIUM):** GeminiCli reclassified to `ContentTier::AgentsMd`. Uses `~/.gemini/AGENTS.md`, not a skills directory. Added `connectors/gemini/AGENTS.md` asset.
- **F6 (MEDIUM):** `install_hooks()` removed from Wave A entirely. No confirmed hook targets outside Claude Code. Dead code from day one.
- **F7 (LOW):** Noted that source skills in `plugin/skills/` use broken `mag hook` calls and must be fixed before adaptation. Wave A gated on parent plan Wave 1 skill rewrites.
- **F8 (LOW):** Uninstall consideration added. `run_uninstall()` must be extended to remove skill dirs and AGENTS.md sections.
- **F9 (LOW):** Three negative-path test cases added to Testing section.
- **F10 (LOW):** Explicit prerequisite added: Wave A depends on parent plan Wave 1 skill rewrites. Effort note added for 0.5 day if fix not yet done.
- **ContentTier enum updated:** `Plugin` renamed to match Claude Code specifically; `AgentsMd` added; `Skills` scoped to OpenCode only.
- Net: ~110 lines → ~160 lines (additions necessary to capture corrected paths and frontmatter spec).

## Change Log

- 2026-03-31: Initial draft
- 2026-03-31: Pass 1 simplification
- 2026-03-31: Pass 2 ground truth pass
- 2026-03-31: Pass 3 — DG R1 incorporated (F1–F10, all HIGH/MEDIUM/LOW findings)
