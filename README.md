# IGOR

IGOR is a planned standalone, embeddable, local-first AI agent runtime. It will connect model providers and multiple MCP servers behind a shared permissions and approval system.

**Status: specification and planning. No runtime has been implemented.** The agreed stack is a Rust core with a Tauri/Vue desktop application. The first demo targets macOS.

## Documentation

- [Documentation index](docs/README.md)
- [Product and demo specification](docs/specs/igor.md)
- [Architecture proposal](docs/specs/architecture.md)
- [Decisions and open questions](docs/specs/decisions.md)
- [Delivery plan](docs/planning/delivery-plan.md)
- [User guide](docs/user-guides/README.md)

The first milestone is a small, reproducible interview demo of the generic runtime. An example purchasing scenario uses fixture-backed sales and weather MCP servers to show multi-server tool use and human approval.

Specifications live in this repository. Agreed implementation tasks will be tracked manually in GitHub Issues after approval. No issues or implementation work are authorised by these documents alone.

## View the docs locally

With Node.js 22 or newer, install dependencies once using `npm install`
(or `pnpm install` / `bun install`). Then run one of:

```sh
npm run docs
pnpm run docs
bun run docs
```

Open the local URL printed in the terminal. VitePress renders the Markdown
as HTML and updates the browser automatically when documentation changes.
Press Ctrl+C to stop. The private development journal is outside the docs site.

Use `npm run docs:build` to generate static HTML in `docs/.vitepress/dist`,
and `npm run docs:preview` to preview that build (equivalent scripts work
with pnpm and Bun).

## Licence

Apache 2.0 has been selected but not yet applied.
