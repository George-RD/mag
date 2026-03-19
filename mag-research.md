# Research Brief — MAG (Memory-Augmented Generation)
Updated: 2026-03-19

## Market Identity
- Core market: Wealth (AI productivity) — power users of 2+ AI tools daily
- Niche: Developers and solopreneurs who lose context switching between AI tools
- Audience specifics: MCP-capable AI tool users (Claude Desktop, Cursor, Claude Code, Windsurf, Cline)

## Starving Crowd Indicators (Phase 1)
- Pain severity evidence: Re-explaining context on every AI tool switch. Having to re-paste docs, re-describe preferences, re-state project context. Users report spending 5-15 min/session on context recovery. GitHub issues across Letta/Mem0 show silent failure and persistence bugs are top pain.
- Purchasing power signals: AI tool power users pay $20-100+/month across tools. Technical users have budget for productivity infra. Railway ~$5/month well within range.
- Targeting specificity: MCP Discord, Claude subreddit, Cursor forums, Hacker News AI threads, GitHub (stars of OpenMemory MCP, AutoMem, Mem0). Specific communities exist but "MCP users" is emerging — not yet a stable mailing list segment.
- Market growth signals: MCP protocol adoption accelerating rapidly (Anthropic, OpenAI, Google all supporting). Memory tools category exploding (Mem0, AutoMem, OpenMemory MCP all launched 2024-2025).
- Score: 28/40 (revised post-adversarial — targeting docked from 7 to 5)

## Value Equation Inputs (Phase 3 — updated)
- Dream outcomes (in customer language):
  - "Open any tool. Your context is already there." (hero — MAG-controllable promise only)
  - "Client data stays on your machine." (privacy trust anchor, cross-persona winner)
  - "Switch from Claude to Cursor. Your context came with you." (dev converter)
  - "Your users get persistent memory. MIT licensed. Runs on their machine." (B2B/embed)
  - DROPPED: "No re-explaining" as standalone — overpromises LLM behavior MAG doesn't control
- Existing proof elements: LoCoMo 90.52% (full 10-sample word-overlap), 500+ tests, Rust, SQLite, MIT + Apache 2.0, hybrid FTS5+semantic+graph, 16 MCP tools
- Speed benchmarks: Sub-ms SQLite ops, <100ms retrieval typical. 134MB model download first run only.
- Effort friction points: Binary install + 5-line MCP config + model download. Non-technical users blocked without Claude plugin.
- Competitor promises:
  - AutoMem: Python, Railway one-click, similar MCP value prop, no graph retrieval
  - OpenMemory MCP: Python, local, MCP, no hybrid retrieval
  - Mem0: managed cloud, API-first, data on their servers

## Phase 3 Adversarial Findings

### Perceived Likelihood gaps (weakest variable)
- Sam: No non-technical demo. Can't find proof it works for someone like her.
- Priya: Needs network-level verification. "I need to see Little Snitch see nothing." Claim vs. verifiable evidence.
- Dave: Needs to read source before trusting. Wants benchmark methodology explained.
- Marketer: Benchmark credibility risk — technical buyers who find benchmark_log.csv and see the scoring-mode changes may question the 90.52% number. AutoMem's benchmark is peer-reviewed; MAG's is internal. Fix: reproducible one-command benchmark runner.

### Effort gaps
- Sam: 2/10. Completely locked out without Claude plugin. Managed waitlist is the bridge.
- Priya: 5/10. Railway OK once she understands "your account, your container." Multi-device gap: "stays on your machine" breaks for her iPad use case.
- Fix: "Your infrastructure, your rules" framing for Railway. Managed waitlist now.

### Dream outcome refinement
- "No re-explaining" removed from primary promise — MAG controls retrieval, not LLM behavior
- Scope promise to: context survives sessions, cross-tool retrieval, privacy
- Priya's framing: "Your client signed an NDA with you. Not with Mem0's infrastructure."

## Offer Landscape (Phase 4)
- Top competitors:
  1. AutoMem — Python, Railway one-click, similar value prop, no graph retrieval, no hybrid FTS5+semantic
  2. OpenMemory MCP (Mem0 OSS) — Python, local, MCP, no hybrid retrieval
  3. Mem0 (managed) — cloud, API-first, data on their servers, paid
- MAG differentiation: Rust (no Python runtime), hybrid FTS5+semantic+graph retrieval, embeddable library, additive-only migrations, MIT + Apache 2.0 stack
- Delivery model: OSS binary/library + Railway template
- Operational costs: near zero (user self-hosts or Railway kickback)
- Customer switching costs: SQLite export/import easy; main lock-in is memory accumulation over time

## Enhancement Data (Phase 5)
- Bonus anchoring values: [TBD in Phase 5]
- Scarcity precedents: none applicable (OSS)
- Guarantee norms: OSS has no guarantee by convention; MIT license disclaimer
- Naming landscape: "MAG" = Memory-Augmented Generation (plays on RAG). Competitors: AutoMem, OpenMemory, Mem0. MAG is distinctive.

## Customer Personas (Phase 3 evolution)

### Solo Sam (Solopreneur)
Phase 0: Non-technical solopreneur, uses Claude Desktop + ChatGPT + others. Doesn't want to think about infrastructure.
Phase 1: Pain 9/10. Would not install via terminal. Needs one-click or plugin.
Phase 2: Free is good. Railway OK at $5/month if she understands it's her account.
Phase 3: Dream 7, Likelihood 3, Delay 5, Effort 2. "Fix the front door. The house is worth it." Claude plugin + waitlist date is the unlock. Managed tier waitlist captures her now.

### Dev Dave (Developer)
Phase 0: Technical developer, Cursor + Claude Code + VS Code. Values performance, hackability.
Phase 1: Pain 8/10. "Zero setup" claim failed — spotted cargo + config = not zero setup. Would install via Homebrew/npm.
Phase 2: Free correct. Railway self-hosting makes sense.
Phase 3: Dream 7, Likelihood 6, Delay 6, Effort 8. Setup is fine. Wants to read source code before trusting. Needs concrete demo of a developer conversation being retrieved. Show the retrieval path on a real tech session, not a toy example.

### Prosumer Priya (Semi-technical)
Phase 0: Privacy-conscious. Client data cannot leave her machine.
Phase 1: Pain 9/10. "Client data stays on your machine" is primary buy signal. Railway OK once she understands it's her account.
Phase 2: Would pay $10/month for managed privacy-respecting option.
Phase 3: Dream 8, Likelihood 5, Delay 6, Effort 5. Needs network verification moment (one-command audit, Little Snitch rule). Multi-device gap: has MacBook + iPad — "your machine" framing breaks. "Your infrastructure, your rules" is more accurate. Plugin timeline matters — "coming later" is not a date.

### OSS Oliver (OSS contributor/maintainer)
Phase 0: Evaluates embeddability and licensing carefully. Would integrate MAG into own project.
Phase 1: Pain 7/10 (less personal, more embed value). Apache 2.0 + MIT = clean commercial use.
Phase 3: [Not reviewed this phase — B2B/embed focus, lower priority for Phase 3]
