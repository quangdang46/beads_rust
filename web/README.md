# br Web UI

A local web UI for **[beads_rust (`br`)](https://github.com/qdang46/beads_rust)** — the agent-first issue tracker.

Forked from [Bead Me Up, Scotty](https://github.com/brendan-appstart/bead-me-up-scotty) by [@brendan-appstart](https://github.com/brendan-appstart). The original was built for the Go-based `bd` CLI; this version is adapted for `br` (Rust) with API routes handled server-side by the `br` binary itself.

## How it works

The web UI is a Next.js static export embedded into the `br` binary. When you run `br web`, the `br` binary starts an HTTP server that:

1. Serves the prebuilt static frontend (HTML, JS, CSS)
2. Implements REST API endpoints that call br's storage library directly

No Node.js, no `npm`, no separate server process — everything is in the single `br` binary.

## Development

```bash
npm install
npm run dev            # http://localhost:3000 (Next.js dev server)
```

The API routes at `app/api/` are available during development but are **not** included in the production build — `br`'s Rust HTTP server handles those.

### Building for production

The static export is built by `scripts/build-web.sh` in the project root:

```bash
bash ../scripts/build-web.sh
```

This copies the output to `src/web/static/` where the Rust `rust-embed` picks it up during `cargo build --features web`.

## License

[MIT](LICENSE) © Brendan (upstream), adapted for beads_rust.
