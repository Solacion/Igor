# IGOR

IGOR is a standalone, embeddable, local-first AI agent runtime. It connects model providers and multiple MCP servers behind a shared permissions and approval system.

**Status: interview demo build.** The Rust core, the `igor-cli` command-line harness and the Tauri/Vue desktop application run on macOS against Anthropic Claude or a local LM Studio model. The demo uses fixture-backed MCP servers; no production MCP server ships with this repository.

## Try it

After [preparing the checkout](docs/user-guides/verification.md):

- **Desktop:** `pnpm run desktop:demo`. See the [desktop guide](docs/user-guides/desktop.md).
- **Command line, no model needed:** the scripted run in the [CLI guide](docs/user-guides/cli.md).

## Documentation

- [Documentation index](docs/README.md)
- [Product and demo specification](docs/specs/igor.md)
- [Architecture proposal](docs/specs/architecture.md)
- [Decisions and open questions](docs/specs/decisions.md)
- [Delivery plan](docs/planning/delivery-plan.md)
- [Pull request review standards](docs/development/review-standards.md)
- [User guide](docs/user-guides/README.md)

The first milestone is a small, reproducible interview demo of the generic runtime. IGOR connects to whichever MCP servers the operator configures, fetches data through their tools, reports back, and performs tasks after approval. An example purchasing scenario uses two fixture-backed MCP servers, sales and stock (standing in for a point-of-sale server) and a supplier catalogue, to show multi-server tool use and human approval.

Specifications live in this repository. The backlog is tracked in [GitHub Issues](https://github.com/Solacion/Igor/issues) as epics, features and user stories. Every pull request into `main` must pass the gated agent code and security reviews.

## View the docs locally

With Node.js 22 or newer and pnpm, install dependencies once using
`pnpm install`. Then run:

```sh
pnpm run docs
```

Open the local URL printed in the terminal. VitePress renders the Markdown
as HTML and updates the browser automatically when documentation changes.
Press Ctrl+C to stop. The private development journal is outside the docs site.

Use `pnpm run docs:build` to generate static HTML in `docs/.vitepress/dist`,
and `pnpm run docs:preview` to preview that build.

## Verify the checkout

Follow the [preparation guide](docs/user-guides/verification.md), then run
`pnpm run verify` for formatting, linting, deterministic tests, dependency-boundary
checks and the docs build. No model API key is needed.

## Licence

IGOR is licensed under the GNU General Public License version 3 only
(`GPL-3.0-only`). See [LICENSE](LICENSE) for the full terms.

Distributed derivatives and combined works must comply with GPLv3. Consider
this when embedding the core in another application. Third-party dependencies
retain their own licences; independently maintained MCP servers are not
relicensed by this repository.
