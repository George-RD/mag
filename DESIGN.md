# MAG design system

## Surface mode

The GitHub Pages landing page is a **Persuade** surface. The README is a **Read** surface that uses the same identity with less visual density.

## Direction

**Local context relay.**

MAG should look like an engineering field log crossed with a compact control-room schematic. The signature image is a useful decision moving from one coding tool, through a local MAG store, into another tool. The design must make cross-tool continuity visible before it explains search architecture.

The identity rejects the category defaults: glowing AI orbs, purple gradients, generic memory dashboards, repeated icon cards and vague “second brain” language.

## Palette

| Token | Value | Role |
|---|---|---|
| Paper | `#f5f7f2` | Main light surface |
| White | `#ffffff` | High-contrast panels |
| Ink | `#0d1b2a` | Text and dark sections |
| Muted ink | `#53677a` | Secondary text |
| Cobalt | `#2457ff` | Context route and primary action |
| Signal orange | `#ff5a36` | Breaks, warnings and comparison |
| Dark signal | `#a82c12` | Orange-family text on light surfaces |
| Live green | `#ccff3d` | Active local state |
| Route tint | `#dce4ff` | MAG comparison column and soft route surfaces |

Use color as fields and signals, not as scattered decoration. Do not use gradients.

## Type

- Display: Archivo Black.
- Body: Atkinson Hyperlegible.
- Code and measurements: JetBrains Mono.
- Safe fallbacks must keep the layout usable when web fonts fail.

Headings are blunt and short. Body copy is plain English. Monospace is reserved for code, labels, measurements and machine state.

## Layout

- Maximum content width: 1180px.
- Use hard rules, offset blocks and asymmetric two-column compositions.
- Keep related content tight and separate major arguments generously.
- Use purpose-built visuals for the relay, retrieval pipeline, benchmark, trade-offs and roadmap.
- Do not make equal cards the page scaffold.

## Components

- **Relay stage:** the core product proof. One decision visibly passes through `~/.mag/memory.db` to another client.
- **Evidence rail:** measured facts with a named benchmark context.
- **Pipeline:** capture, store, recall and explain.
- **Comparison table:** highlights fit rather than claiming universal superiority.
- **Benchmark board:** MAG and AutoMem shown at the same scale with the recall caveat nearby.
- **Roadmap rail:** issue-linked directions with no invented dates.
- **Install terminal:** a real command and a visible copy action.

## Motion

One restrained relay-dot sequence is allowed. The page must remain complete before animation starts. Disable non-essential motion under `prefers-reduced-motion`.

## Responsive behavior

- At narrow widths, stack narrative and proof without changing their order.
- The relay becomes a vertical route.
- The comparison table remains horizontally scrollable inside a labelled, keyboard-focusable region.
- Primary actions and issue links keep at least a 44px touch target.
- The document itself must not create horizontal page scroll.

## Accessibility and performance

- Body text contrast must meet WCAG AA.
- Keep a visible two-layer focus treatment on interactive controls.
- Use semantic landmarks, one `h1`, ordered headings and a skip link.
- The site remains static HTML, CSS and small vanilla JavaScript.
- No framework or runtime dependency.
- Social and README visuals must include meaningful titles, descriptions or alt text.

## README visuals

README SVGs use fixed GitHub-safe colors and system fonts. They show the same relay mechanism without scripts, external assets or motion. Copy remains primary; visuals orient the reader and explain the mechanism.

## Change rule

Product claims come from `PRODUCT.md` and repository evidence. Visual changes should preserve the context-relay idea unless the positioning changes. A clean detector result does not justify generic design, and a bold visual does not justify an unsupported claim.
