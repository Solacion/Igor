# User guide

IGOR is currently in planning. There is no installable application or verified startup procedure yet.

## Intended demo workflow

1. Open IGOR with the configured model provider and MCP server connections.
2. Confirm connection status.
3. Make a request, such as the example "Create an order for supplier X", which uses sales and weather data.
4. Answer any clarifying questions, such as the order date or period; IGOR asks rather than assumes.
5. Inspect tool activity, evidence, assumptions and the proposed action with its exact arguments.
6. Approve or deny the action. Write-classified tools run only after explicit approval.

In the demo, the approved action is `create_draft_order`, which creates a draft only and changes no external system. The same flow can be run headlessly with `igor-cli`.

## Guides to add with the implementation

- Installation and supported platforms.
- Configuration, credentials and MCP-server startup.
- Running the reproducible sample demo.
- Understanding permissions, approvals and local data storage.
- Troubleshooting unavailable providers, servers and incomplete results.

See the [specification](../specs/igor.md) for intended behaviour and the [open decisions](../specs/decisions.md) for choices still requiring agreement.
