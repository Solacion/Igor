import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import { PERIODS } from './periods.js';
const READ_ONLY = { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false };
const period = {
    period: z.enum(PERIODS).optional().describe('Named period in the shop\'s local time; the server knows today\'s date. Defaults to today. Use this for today, yesterday, this week, last month and similar.'),
    from: z.string().optional().describe('First day, YYYY-MM-DD. Only for dates the operator stated; never work out today\'s date yourself. Overrides period. A range covers at most 366 days.'),
    to: z.string().optional().describe('Last day, YYYY-MM-DD. Defaults to today when from is given.'),
};
/** Turns a tool's result or error into MCP content. Errors are reported to the model, not thrown. */
async function respond(work) {
    try {
        return { content: [{ type: 'text', text: JSON.stringify(await work()) }] };
    }
    catch (err) {
        return { isError: true, content: [{ type: 'text', text: err instanceof Error ? err.message : String(err) }] };
    }
}
/** The POS MCP server: read-only tools over the RedTill database. */
export function createServer(tools, version) {
    const server = new McpServer({ name: 'redtill-pos', version }, { instructions: 'Read-only access to the shop\'s RedTill POS: products, stock on hand, sales and supplier orders. Money is in rand; sales amounts include VAT. For relative days such as today or last week, pass period and let the server work out the dates; results include today\'s date. Product names are often Afrikaans: search with the words the operator used, and try a shorter word if nothing matches. Stock on hand can be negative when sales were rung up for stock never received in the POS; say so rather than treating it as a real shortage.' });
    server.registerTool('search_products', {
        title: 'Search products',
        description: 'Find products by name words, item code or barcode. Returns price and cost (excluding and including VAT), margin and total stock on hand.',
        inputSchema: {
            query: z.string().min(1).max(100).describe('Words from the product name, or an exact item code or barcode.'),
            limit: z.number().int().min(1).max(50).optional().describe('Most products to return; default 20.'),
        },
        annotations: { title: 'Search products', ...READ_ONLY },
    }, args => respond(() => tools.searchProducts(args)));
    server.registerTool('get_stock_levels', {
        title: 'Stock levels',
        description: 'Stock on hand in total and by location, with warning level, reorder quantity and order pack. Use low_only for products at or below their warning level (what needs reordering).',
        inputSchema: {
            query: z.string().max(100).optional().describe('Words from the product name, or an exact item code or barcode.'),
            product_ids: z.array(z.number().int()).max(50).optional().describe('Product ids from search_products.'),
            department: z.string().max(100).optional().describe('Part of a department name, such as Bakery or Dranke.'),
            low_only: z.boolean().optional().describe('Only products at or below their warning level.'),
            limit: z.number().int().min(1).max(100).optional().describe('Most products to return; default 25.'),
        },
        annotations: { title: 'Stock levels', ...READ_ONLY },
    }, args => respond(() => tools.stockLevels(args)));
    server.registerTool('get_sales_summary', {
        title: 'Sales summary',
        description: 'Number of sales, revenue including VAT and average sale for a period, optionally broken down by day, department or category. Only paid and account sales count.',
        inputSchema: {
            ...period,
            group_by: z.enum(['total', 'day', 'department', 'category']).optional().describe('Breakdown; default total.'),
        },
        annotations: { title: 'Sales summary', ...READ_ONLY },
    }, args => respond(() => tools.salesSummary(args)));
    server.registerTool('get_top_products', {
        title: 'Top products',
        description: 'Best-selling products for a period by revenue (including VAT) or quantity, optionally within one department.',
        inputSchema: {
            ...period,
            by: z.enum(['revenue', 'quantity']).optional().describe('Rank by; default revenue.'),
            department: z.string().max(100).optional().describe('Part of a department name.'),
            limit: z.number().int().min(1).max(50).optional().describe('Most products to return; default 10.'),
        },
        annotations: { title: 'Top products', ...READ_ONLY },
    }, args => respond(() => tools.topProducts(args)));
    server.registerTool('get_product_sales_history', {
        title: 'Product sales history',
        description: 'Daily sales of one product over recent days, with stock on hand, average sold per day and roughly how many days the stock will last. Use it to decide how much to reorder.',
        inputSchema: {
            product_id: z.number().int().describe('Product id from search_products.'),
            days: z.number().int().min(1).max(366).optional().describe('Days to look back, including today; default 28.'),
        },
        annotations: { title: 'Product sales history', ...READ_ONLY },
    }, args => respond(() => tools.productSalesHistory(args)));
    server.registerTool('list_suppliers', {
        title: 'Suppliers',
        description: 'Active suppliers with their code and number of active products.',
        inputSchema: {
            query: z.string().max(100).optional().describe('Words from the supplier name.'),
            limit: z.number().int().min(1).max(150).optional().describe('Most suppliers to return; default 50.'),
        },
        annotations: { title: 'Suppliers', ...READ_ONLY },
    }, args => respond(() => tools.listSuppliers(args)));
    server.registerTool('get_supplier_orders', {
        title: 'Supplier orders',
        description: 'Recent supplier orders with status (created, received, completed or cancelled), dates and quantities ordered and received.',
        inputSchema: {
            supplier_id: z.number().int().optional().describe('Supplier id from list_suppliers.'),
            status: z.enum(['created', 'received', 'completed', 'cancelled']).optional(),
            days: z.number().int().min(1).max(366).optional().describe('Days to look back; default 90.'),
            limit: z.number().int().min(1).max(100).optional().describe('Most orders to return; default 20.'),
        },
        annotations: { title: 'Supplier orders', ...READ_ONLY },
    }, args => respond(() => tools.supplierOrders(args)));
    return server;
}
