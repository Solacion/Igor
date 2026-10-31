# User guide

IGOR runs as a desktop application or as the `igor-cli` command-line harness. Both use the same core, the same configuration file and the same approval rules.

## Start here

1. [Prepare and verify a checkout](verification.md): prerequisites, dependencies and the deterministic checks.
2. [Configuration](configuration.md): model providers, MCP servers, secrets, tool permissions and limits.
3. Run the demo:
   - [Desktop](desktop.md): `pnpm run desktop:demo`, with the click-by-click demo script and fixture facts to check.
   - [Command line](cli.md): the scripted offline run, or a live provider.
4. [Troubleshooting and cleanup](troubleshooting.md): unavailable providers and servers, missing keys, and removing local state.

## Demo workflow

1. Open IGOR with the configured model provider and MCP server connections.
2. Confirm connection status.
3. Make a request, such as "Create an order for supplier X", which uses sales, stock and supplier catalogue data from two servers.
4. Answer any clarifying questions, such as the order date or period; IGOR asks rather than assumes.
5. Inspect tool activity and the proposed action with its exact arguments.
6. Approve or deny the action. Write-classified tools run only after explicit approval.

In the demo, the approved action is `create_draft_order`, which creates a draft only and changes no external system.

## Models

- **LM Studio (local, no cost):** load `qwen/qwen3-vl-8b` and start LM Studio's local server at `http://127.0.0.1:1234/v1`. Verified on an Apple M4 Max with 48 GB; the 4-bit MLX build needs about 6 GB of free memory, so 16 GB of RAM is a practical minimum. Smaller models may not call tools reliably.
- **Anthropic Claude:** set `ANTHROPIC_API_KEY` in the environment that starts IGOR. Requests are charged to that key.

See the [specification](../specs/igor.md) for intended behaviour and the [open decisions](../specs/decisions.md) for choices still requiring agreement.
