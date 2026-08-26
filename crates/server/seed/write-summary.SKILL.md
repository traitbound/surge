---
name: write-summary
description: Produce a concise, accurate summary document of the current repository for downstream pipeline nodes. Use when a doc node's declared output is a summary.
---

# write-summary

Write a summary document that lets a downstream agent act on this repository
without re-reading all of it. The doc node declares where it lands (e.g.
`docs/summary.md`); write exactly that file.

## Method

1. **Survey.** Read the repository top-down: the readme or root docs, the
   manifest(s), the directory layout. Note the languages, entry points and
   build tooling actually present.
2. **Trace one real path.** Follow one representative flow end to end (e.g.
   request → handler → store) so the summary reflects how the code actually
   fits together, not just what the docs claim.
3. **Write the summary** with these sections, in order:
   - **Purpose** — what the project does, in two or three sentences.
   - **Layout** — the significant directories and what lives in each, as a
     short table of relative paths.
   - **How it runs** — build, test and run commands that exist today.
   - **Points of interest** — the three to five files a newcomer should read
     first, each with one line on why.
4. **Verify.** Every path you name must exist; every command must come from
   the repo's own tooling files. Delete any sentence you cannot ground.

## Style

Plain prose, present tense, no marketing language. Prefer a short table to a
long paragraph. The whole document should read in under five minutes.

## Reporting

Emit progress through the three surge MCP reporting tools the compiled
runtime registers for you — everything you need is here, and there is no
documentation file to go and read (the bound repo's read list is closed,
INV-DATA-6). (`surge_fetch_work_order` is also registered, but a doc run
holds no issue and so has no work order to fetch — do not call it.)

- `surge_append_span` — required `body`; optional `status` (`ok` · `error` ·
  `refused`, default `ok`), `role`, `node_id`, `duration_ms`, `cost`. Spans
  are append-only — nothing closes or amends one — so append a finished span
  after each step of the method above rather than opening one when a step
  begins; a span left `running` never stops being `running`.
- `surge_heartbeat` — no arguments; call it through long survey reads so the
  lease clock does not mistake reading for silence.
- `surge_poll_run` — no arguments; call it before each step so an abort can
  land between phases.
