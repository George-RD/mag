# Writing style

This guide governs prose in `README.md`, `docs/`, `meta/todos/`, commit messages,
and code comments. It exists because most of MAG's documentation is now drafted
by language models, and the default register of those models is wrong for this
project: confident where the evidence is thin, decorative where it should be
specific, and long where a sentence would do.

Apply it to any text a reader will judge the project by.

## The standard

Write like an engineer explaining a system to another engineer who will have to
maintain it. State what the software does, what it does not do, and what it
costs. A reader should finish a page knowing something they can act on.

## Rules

### Say the thing

Lead with the fact. Do not warm up.

> **No.** In today's rapidly evolving AI landscape, developers face a growing
> challenge when it comes to managing context across tools.
>
> **Yes.** Built-in memory stays inside one product. Switching tools loses it.

Cut any sentence that only announces what the next sentence will say. Cut any
closing paragraph that restates the section above it.

### Prefer the concrete noun

Numbers, file paths, commands, flag names, field names, error text. If you
cannot name the thing, you do not yet understand it well enough to document it.

> **No.** MAG offers powerful search capabilities across your data.
>
> **Yes.** `mag advanced-search` combines FTS5, ONNX embeddings and graph
> neighbours, then abstains when the top score falls below the cutoff.

### Banned vocabulary

These words signal machine drafting and almost never carry information:

`delve`, `leverage` (as a verb), `robust`, `seamless`, `seamlessly`,
`comprehensive`, `powerful`, `cutting-edge`, `state-of-the-art`,
`game-changing`, `revolutionise`, `elevate`, `unlock`, `harness` (as a verb),
`streamline`, `boasts`, `empower`, `effortless`, `blazing fast`, `rich set of`,
`wide range of`, `a variety of`, `plethora`, `myriad`, `crucial`, `vital`,
`essential` (as filler), `notably`, `arguably`, seemingly, `it's worth noting
that`, `at the end of the day`, `in today's ... world`.

Also banned as constructions:

- **The false contrast.** "It's not just a database, it's a memory system."
- **The audience fan-out.** "Whether you're a solo developer or a large team..."
- **Forced triads.** Three adjectives where one would do. Three-item lists
  padded to three.
- **The rhetorical question opener.** "So what does this actually mean?"
- **Bolded key phrases** scattered through a paragraph for emphasis.
- **Emoji.** None, anywhere, including headings and tables.

### Hedge honestly, not defensively

Real uncertainty gets stated plainly and specifically. Deterministic behaviour
does not get hedged.

> **No.** MAG can help you potentially improve context retention across sessions.
>
> **Yes.** MAG stores memories in `~/.mag/memory.db`. Retrieval quality on
> workloads unlike LoCoMo is not measured.

Never write "aims to", "is designed to", or "should" about behaviour that is
already implemented and testable. Write what it does.

### Claims carry their evidence

A performance number needs its dataset, date, commit and command, or a link to
the page that has them. A comparison names what was and was not held constant.
If a result has a known limitation, the limitation goes next to the result, not
in a footnote.

### Structure

- One idea per paragraph. Three sentences is usually enough.
- Headings are noun phrases or imperatives, not questions.
- Tables for comparisons and reference material. Prose for reasoning.
- Code blocks are runnable as written. No `...` placeholders inside a command a
  reader is meant to copy.
- Use British or American spelling to match the file you are editing. Do not
  change an existing file's convention. Code and code comments use `-ize`.

### Voice

Second person for instructions ("run `mag setup`"). Present tense. Active voice
unless the actor genuinely does not matter. Do not use "we" to mean the
software.

## Check the draft before you commit

Read it back and ask:

1. Which sentence here could be deleted without losing information? Delete it.
2. Which adjective is doing no work? Cut it.
3. Which claim would I be embarrassed by if a user tested it? Qualify or remove
   it.
4. Does any paragraph exist only to fill a section heading? Remove the heading.
