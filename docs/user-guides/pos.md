# RedTill POS server

`apps/pos-mcp` is an MCP server that lets IGOR answer questions from a RedTill point-of-sale database on SQL Server: sales, stock on hand, products, suppliers and supplier orders. This first version is read-only; drafting supplier orders is planned (#145).

## Tools

| Tool | Answers |
|---|---|
| `search_products` | Products by name words, item code or barcode, with selling price and cost (excluding and including VAT), margin and stock on hand. |
| `get_stock_levels` | Stock on hand in total and by location, with warning level, reorder quantity and order pack. `low_only` lists what needs reordering. |
| `get_sales_summary` | Number of sales, revenue including VAT and average sale for a period, overall or by day, department or category. |
| `get_top_products` | Best sellers for a period, by revenue or quantity. |
| `get_product_sales_history` | Daily sales of one product, average per day and roughly how many days the stock will last. |
| `list_suppliers` | Active suppliers and how many products each supplies. |
| `get_supplier_orders` | Recent supplier orders: created, received, completed or cancelled. |

Periods are named (`today`, `yesterday`, `this_week`, `last_7_days`, `this_month`, `last_month`, `last_30_days`, `this_year`) in the shop's local time, so the model does not need to know today's date. A custom range covers at most 366 days.

## How it reads RedTill

- Only paid sales (status 9) and account sales (status 4) count as sales. Quotes, open sales, jobs, bookings, voided and cancelled sales are left out.
- Sales totals include VAT, as RedTill stores them. Selling prices are stored excluding VAT; the server adds the product's VAT rate.
- Whether cost prices include VAT is read from RedTill's `CostIncludesTax` setting, checked again every five minutes.
- Stock on hand is the total over all locations, with a per-location split.
- Stock can be negative when sales were rung up for stock never received in the POS. The server tells the model to say so rather than call it a shortage.
- Customer names, phone numbers and card details are never returned.

## Safety

- Every query is fixed and parameterised; there is no tool that runs free-form SQL.
- Each query times out after 15 seconds and returns at most 200 rows. The server opens at most two connections.
- Use a SQL login that can only read. The server never writes in this version, but the database should refuse writes regardless.

## Configure IGOR

Build the server once:

```sh
pnpm install --frozen-lockfile
pnpm run pos:build
```

Add it to the configuration file. Connection settings are arguments; the password is a secret reference, never a value in the file:

```toml
[[servers]]
id = "pos"
transport = "stdio"
command = "/opt/homebrew/bin/node"
args = ["/path/to/Igor/apps/pos-mcp/dist/index.js",
        "--server", "pos.example.lan", "--port", "1433",
        "--database", "redtill", "--user", "readonly_login"]
env = { POS_DB_PASSWORD = { env = "POS_DB_PASSWORD" } }

[permissions.pos]
search_products = "read"
get_stock_levels = "read"
get_sales_summary = "read"
get_top_products = "read"
get_product_sales_history = "read"
list_suppliers = "read"
get_supplier_orders = "read"
```

`command` must be the full path to Node.js 22 or later (`which node`).

### Old SQL Server versions

SQL Server 2014 before SP1 CU5 only speaks TLS 1.0/1.1, which current Node.js refuses. Add `--legacy-tls` to allow those versions for this connection only, and only when the database is reached over a VPN or a trusted private network. Updating SQL Server to support TLS 1.2 is the better fix.

## Check it

```sh
pnpm run pos:check
```

This type-checks the server and runs its unit tests, which use a fake database. They check that search words are always parameters, row limits are capped, only paid and account sales count, VAT follows the POS settings, and periods resolve correctly.
