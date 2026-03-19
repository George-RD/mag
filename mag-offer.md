# MAG (Memory-Augmented Generation) — Grand Slam Offer Workshop
Generated: 2026-03-19
Status: Phase 3 of 5 complete

## Phase 0: Discovery
- Business: MAG — Rust MCP memory server. SQLite + ONNX embeddings (bge-small-en-v1.5, 384-dim). 16 MCP tools exposed via stdio. 500+ tests. MIT licensed. No external services required.
- Market: AI tool power users — solopreneurs, developers, and technical prosumers who use multiple AI tools daily and lose context switching between them.
- Personas:
  - Solo Sam — solopreneur, uses many AI tools (Claude, ChatGPT, etc.), non-technical, dabbled with Claude Desktop
  - Dev Dave — developer, technical, uses Cursor/Claude Code/VS Code, values performance and hackability
  - Prosumer Priya — semi-technical, uses AI tools for creative/professional work, cares about privacy
  - OSS Oliver — open source contributor/maintainer, evaluates embeddability and licensing carefully

## Phase 1: Starving Crowd
- Market scores: Pain 9/10, Purchasing Power 7/10, Targeting 7/10, Growth 9/10 = 32/40 raw
- Post-adversarial revised: 28/40 (targeting docked — "MCP users" is still emerging segment, not a lists-based audience)
- Core market: Wealth (productivity) + adjacent to Relationships (AI as collaborative partner)
- Niche: Power users of 2+ AI tools daily who lose context on every switch — developers and solopreneurs who rely on AI for work output
- 3 must-fix items identified post-adversarial:
  1. "Zero setup" claim is false — fix setup or fix the claim.
  2. Differentiate vs OpenMemory MCP in first paragraph — same value prop, must state MAG's advantages explicitly (hybrid FTS5+semantic+graph, Rust/embeddable, no Python runtime, additive migrations)
  3. Non-technical user path unclear — Homebrew/npm/Claude plugin needed (GitHub issue filed)
- 3 decisions:
  1. bge-small (Apache 2.0) locked as sole default embedding model — voyage-4-nano rejected (proprietary licensing risk)
  2. Distribution: local binary + Homebrew + npm + Claude plugin + Railway one-click (issue filed for packaging)
  3. Cross-tool memory positioning: "common agent memory across different AI tools" is the core value prop

## Phase 2: Pricing
- Price: $0 (free product)
- Position: Free Value Leader
- 10x challenge: moot at $0 — 10x of free is still free
- Revenue model: Railway OSS kickback 30-50% of compute spend (~$5-10/month user cost)
- Future: managed hosting if demand warrants
- 3 deployment tiers:
  1. Local binary — zero cloud, zero cost, runs on your machine
  2. Self-hosted (Railway) — one-click deploy, user's own Railway account, ~$5/month, same privacy guarantee
  3. Managed (coming soon) — hosted by MAG team
- Railway framing: "Self-hosted in one click. Your infrastructure, your rules."

## Phase 3: Value Equation

### Adversarial Scores
| Agent | Dream | Likelihood | Delay | Effort |
|---|---|---|---|---|
| Solo Sam | 7 | 3 | 5 | 2 |
| Dev Dave | 7 | 6 | 6 | 8 |
| Prosumer Priya | 8 | 5 | 6 | 5 |
| Marketer | — | — | — | 5/10 overall |
| Strategist | — | — | — | 3/10 biz model |

### Dream Outcome — 7.5/10
- Hero: "Open any tool. Your context is already there." (drop "No re-explaining" as standalone — overpromises LLM behavior MAG doesn't control)
- Monday Morning scene: "Open Claude on Monday. It already knows what you decided on Friday."
- Privacy frame: "Your client signed an NDA with you. Not with Mem0's infrastructure."
- Universal: "Stop being your AI's memory. That's not your job."
- B2B/embed: "Your users get persistent memory. Ship it Friday. Never touch it again."

### Perceived Likelihood — 5/10 (weakest variable)
Primary fixes selected:
- "Don't trust our number. Run it." — publish one-command LoCoMo benchmark runner, challenge to reproduce 90.52%
- Compatibility matrix: tool-by-tool checkmarks on landing page (Claude Desktop, Cursor, Claude Code, Windsurf, Cline)
- "We can't deprecate your local binary. It runs whether we exist or not."
- Migration proof from AutoMem/OpenMemory MCP — config diff, 12 minutes, documented
- Network transparency: one-command audit showing zero outbound network calls (Priya's verification moment)
- Lock benchmark methodology: full 10-sample word-overlap, pinned commit hash, public reproduction script

### Time Delay — 6/10
- Define 48-hour first win in onboarding: "Open a new session. Ask about something from your last session. Watch it recall. That's the moment."
- Unedited "Live in 5" screen recording — fresh install to first cross-session recall, no cuts, show the clock
- Railway as primary non-tech CTA: "Running in 90 seconds. No terminal."
- First-run transparency: "134MB model download on first launch. ~30 seconds. Then instant forever."

### Effort & Sacrifice — 4/10 (critical gap)
- Pre-filled MCP config snippet per tool — OS-aware, copy-paste only, no path guessing
- Railway path: "No code. No terminal. Click a button. Your infrastructure, your rules."
- Railway multi-device frame: "Access from any device. Data lives in your Railway account. No one else's."
- Managed tier waitlist NOW as DFY anchor: "Not ready to self-host? Join the waitlist. You're first in line."
- Claude plugin: primary CTA for non-technical users with waitlist + target date (most important unlock for Sam)

### Critical decisions
1. Dream outcome scoped to MAG's control — retrieval promise, not LLM behavior promise
2. Benchmark: publish reproducible runner, address methodology proactively in README
3. Railway copy: "your infrastructure, your rules" — not "your machine" (multi-device problem)
4. Managed tier waitlist is blocking conversion for Sam — ship the waitlist page
5. Strategist: Railway kickback alone (~$1.4k/month at 10k installs) is not a business. 90-day prescription: managed tier waitlist, 10 paying users at $15/month to validate thesis.

## Phase 4: Offer Stack
[Pending]

## Phase 5: Enhancement
[Pending]

## Copy Decisions (Cross-Phase)
- Hero headline: "Open any tool. Your context is already there."
- Trust anchor: "Client data stays on your machine. Not on Mem0's servers. Not anyone else's."
- Dev section header: "Switch from Claude to Cursor. Your context came with you."
- README/embed only: "Your users get persistent memory. MIT licensed. Runs on their machine. No extra services."
- Railway framing: "Self-hosted in one click. Your infrastructure, your rules."
- DROPPED: "No re-explaining" as standalone — overpromises LLM behavior
