import sql from 'mssql';
/** Bound on one query; the POS is a live system. */
export const QUERY_TIMEOUT_MS = 15_000;
/** Most rows any query may return to a tool. */
export const MAX_ROWS = 200;
export class DbError extends Error {
}
/** A connection pool of at most two connections, opened on first use. */
export function connect(config) {
    let pool;
    const open = () => {
        pool ??= new sql.ConnectionPool({
            server: config.server,
            port: config.port,
            database: config.database,
            user: config.user,
            password: config.password,
            requestTimeout: QUERY_TIMEOUT_MS,
            connectionTimeout: 10_000,
            pool: { min: 0, max: 2, idleTimeoutMillis: 60_000 },
            options: {
                encrypt: true,
                // POS servers on a private network rarely have a certificate a client can verify.
                trustServerCertificate: true,
                // Dates are the shop's local time; read them as written.
                useUTC: false,
                appName: 'igor-pos-mcp',
                ...(config.legacyTls ? { cryptoCredentialsDetails: { minVersion: 'TLSv1', ciphers: 'DEFAULT@SECLEVEL=0' } } : {}),
            },
        }).connect().catch((err) => {
            pool = undefined;
            throw new DbError(`cannot connect to the POS database: ${message(err, config.password)}`);
        });
        return pool;
    };
    return {
        async query(text, params = []) {
            const request = (await open()).request();
            for (const param of params) {
                const type = param.type === 'int' ? sql.Int : param.type === 'bit' ? sql.Bit : sql.VarChar(400);
                request.input(param.name, type, param.value);
            }
            try {
                const result = await request.query(text);
                return result.recordset.slice(0, MAX_ROWS);
            }
            catch (err) {
                throw new DbError(`the POS database query failed: ${message(err, config.password)}`);
            }
        },
        async close() {
            if (pool)
                await (await pool).close();
        },
    };
}
/** An error message with the password removed, should a driver ever echo it. */
function message(err, password) {
    const text = err instanceof Error ? err.message : String(err);
    return password ? text.split(password).join('[redacted]') : text;
}
