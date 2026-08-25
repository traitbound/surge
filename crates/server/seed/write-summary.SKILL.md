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

While working, emit progress via the surge MCP tools (`surge_append_span`,
`surge_heartbeat`, `surge_poll_run`) as described in
`integrations/claude-plugin/README.md`, so the run's span tree shows this
step and an abort can land between phases.
