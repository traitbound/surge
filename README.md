# Surge

A local, single-user delivery pipeline service: Rust/Axum, embedded SQLite, a React UI, and supervised Codex workers. It compiles pipeline content into runtime files and records runs/spans. General graph execution, the canvas and full boards remain planned work.

Start with [the current roadmap](docs/phases/README.md). Phase 0 has historical Claude acceptance; **Phase 0.1 is next** to validate the current Codex integration and browser loop. Phase plans contain acceptance criteria, not implementation claims.

- [Product spec](docs/product/spec.md) and [architecture](docs/product/architecture.md)
- [Invariants](docs/product/invariants.md) and [feature readiness](docs/features/INDEX.md)
- [Detailed design](docs/design.md) — latest dated §23 resolutions override prototype copy
- [Development instructions](CLAUDE.md) and [operating contract](ENGAGEMENT.md)

## Local checks

```sh
SQLX_OFFLINE=true cargo test --workspace
cd ui
npm ci
npm run build
```

UI/plugin assets are embedded in the service binary. Worker integration still requires Codex, Node.js, git and hook utilities. Current workers have network access enabled; Surge does not enforce an egress allowlist. Runtime setup and real-worker acceptance belong to Phase 0.1.
