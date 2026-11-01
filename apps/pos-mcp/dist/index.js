#!/usr/bin/env node
// Igor launches this over stdio. Diagnostics go to stderr; stdout carries only MCP messages.
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { ConfigError, readConfig } from './config.js';
import { connect } from './db.js';
import { createServer } from './server.js';
import { PosTools } from './tools.js';
const VERSION = '0.1.0';
async function main() {
    const config = readConfig(process.env, process.argv.slice(2));
    const db = connect(config);
    const server = createServer(new PosTools(db), VERSION);
    const shutdown = async () => {
        await server.close().catch(() => { });
        await db.close().catch(() => { });
        process.exit(0);
    };
    process.on('SIGINT', shutdown);
    process.on('SIGTERM', shutdown);
    process.stdin.on('end', shutdown);
    await server.connect(new StdioServerTransport());
}
main().catch(err => {
    console.error(err instanceof ConfigError ? `redtill-pos: ${err.message}` : err);
    process.exit(1);
});
