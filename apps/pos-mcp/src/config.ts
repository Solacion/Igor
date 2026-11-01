/** Connection settings. Igor passes the password as an environment variable and the rest as
 * arguments (`--server`, `--port`, `--database`, `--user`, `--legacy-tls`) or variables. */
export interface PosConfig {
  server: string
  port: number
  database: string
  user: string
  password: string
  /** Allow TLS 1.0/1.1 for this connection only. For old SQL Server builds reached over a VPN. */
  legacyTls: boolean
}

export class ConfigError extends Error {}

const FLAGS = { '--server': 'POS_DB_SERVER', '--port': 'POS_DB_PORT', '--database': 'POS_DB_DATABASE', '--user': 'POS_DB_USER' } as const

/** Settings from `args` and `env`; arguments win. The password comes only from `env`, is required
 * and is never logged. */
export function readConfig(env: Record<string, string | undefined>, args: string[] = []): PosConfig {
  const values: Record<string, string | undefined> = { ...env }
  for (let i = 0; i < args.length; i++) {
    const arg = args[i]!
    if (arg === '--legacy-tls') {
      values.POS_DB_LEGACY_TLS = '1'
      continue
    }
    const name = FLAGS[arg as keyof typeof FLAGS]
    if (!name || args[i + 1] === undefined)
      throw new ConfigError(`unknown or incomplete argument ${JSON.stringify(arg)}`)
    values[name] = args[++i]
  }
  const required = (name: string) => {
    const value = values[name]?.trim()
    if (!value)
      throw new ConfigError(name === 'POS_DB_PASSWORD' ? 'set POS_DB_PASSWORD' : `pass --${name.slice(7).toLowerCase()} or set ${name}`)
    return value
  }
  const port = Number(values.POS_DB_PORT ?? '1433')
  if (!Number.isInteger(port) || port < 1 || port > 65535)
    throw new ConfigError('POS_DB_PORT must be a port number')
  return {
    server: required('POS_DB_SERVER'),
    port,
    database: required('POS_DB_DATABASE'),
    user: required('POS_DB_USER'),
    password: required('POS_DB_PASSWORD'),
    legacyTls: values.POS_DB_LEGACY_TLS === '1' || values.POS_DB_LEGACY_TLS === 'true',
  }
}
