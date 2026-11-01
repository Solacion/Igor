import { describe, expect, it } from 'vitest'
import { readConfig } from './config.js'
import type { Db, Param } from './db.js'
import { previousRange, resolveRange, sqlDate } from './periods.js'
import { PosTools } from './tools.js'

/** A database that records each query and answers from `reply`. */
function fakeDb(reply: (sql: string, params: Param[]) => object[] = () => []) {
  const calls: { sql: string, params: Param[] }[] = []
  const db: Db = {
    async query<T extends object>(sql: string, params: Param[] = []) {
      calls.push({ sql, params })
      return reply(sql, params) as T[]
    },
    async close() {},
  }
  return { db, calls }
}

const sunday = new Date(2026, 10, 1, 14, 30)
const param = (params: Param[], name: string) => params.find(p => p.name === name)?.value

describe('periods', () => {
  it('resolves named periods in local time, weeks starting on Monday', () => {
    expect(resolveRange({}, sunday)).toEqual({ from: '2026-11-01', to: '2026-11-01' })
    expect(resolveRange({ period: 'yesterday' }, sunday)).toEqual({ from: '2026-10-31', to: '2026-10-31' })
    expect(resolveRange({ period: 'this_week' }, sunday)).toEqual({ from: '2026-10-26', to: '2026-11-01' })
    expect(resolveRange({ period: 'last_month' }, sunday)).toEqual({ from: '2026-10-01', to: '2026-10-31' })
    expect(resolveRange({ period: 'last_30_days' }, sunday)).toEqual({ from: '2026-10-03', to: '2026-11-01' })
  })

  it('checks custom ranges', () => {
    expect(resolveRange({ from: '2026-10-01' }, sunday)).toEqual({ from: '2026-10-01', to: '2026-11-01' })
    expect(() => resolveRange({ from: '2026-02-30' }, sunday)).toThrow(/not a date/)
    expect(() => resolveRange({ from: '2026-11-02', to: '2026-11-01' }, sunday)).toThrow(/before/)
    expect(() => resolveRange({ from: '2024-01-01', to: '2026-01-01' }, sunday)).toThrow(/366/)
    expect(() => resolveRange({ from: '2026-11-02', to: '2026-11-03' }, sunday)).toThrow('after today, 2026-11-01')
    expect(resolveRange({ from: 'yesterday' }, sunday)).toEqual({ from: '2026-10-31', to: '2026-10-31' })
    expect(() => resolveRange({ from: 'last week' }, sunday)).toThrow(/pass period/)
    expect(sqlDate('2026-11-01')).toBe('20261101')
  })
})

describe('previous periods', () => {
  it('compares with the same days of the previous week, month or year', () => {
    const range = (period: Parameters<typeof resolveRange>[0]['period']) => previousRange(resolveRange({ period }, sunday), period)
    expect(range('today')).toEqual({ from: '2026-10-31', to: '2026-10-31' })
    expect(range('this_week')).toEqual({ from: '2026-10-19', to: '2026-10-25' })
    expect(range('last_7_days')).toEqual({ from: '2026-10-19', to: '2026-10-25' })
    expect(range('this_month')).toEqual({ from: '2026-10-01', to: '2026-10-01' })
    expect(range('last_month')).toEqual({ from: '2026-09-01', to: '2026-09-30' })
    expect(range('this_year')).toEqual({ from: '2025-01-01', to: '2025-11-01' })
    expect(previousRange({ from: '2026-03-01', to: '2026-03-31' }, 'this_month')).toEqual({ from: '2026-02-01', to: '2026-02-28' })
    expect(previousRange({ from: '2026-10-01', to: '2026-10-10' })).toEqual({ from: '2026-09-21', to: '2026-09-30' })
  })
})

describe('config', () => {
  it('needs the connection settings and keeps legacy TLS off unless asked', () => {
    const env = { POS_DB_SERVER: 'pos', POS_DB_DATABASE: 'redtill', POS_DB_USER: 'reader', POS_DB_PASSWORD: 'secret' }
    expect(readConfig(env)).toMatchObject({ server: 'pos', port: 1433, legacyTls: false })
    expect(readConfig({ ...env, POS_DB_PORT: '11433', POS_DB_LEGACY_TLS: '1' })).toMatchObject({ port: 11433, legacyTls: true })
    expect(() => readConfig({ ...env, POS_DB_PASSWORD: ' ' })).toThrow('set POS_DB_PASSWORD')
    expect(() => readConfig({ ...env, POS_DB_PORT: 'x' })).toThrow(/port/)
    const args = ['--server', '10.0.0.5', '--port', '11433', '--database', 'redtill', '--user', 'reader', '--legacy-tls']
    expect(readConfig({ POS_DB_PASSWORD: 'secret' }, args)).toEqual({ server: '10.0.0.5', port: 11433, database: 'redtill', user: 'reader', password: 'secret', legacyTls: true })
    expect(() => readConfig({ POS_DB_PASSWORD: 'secret' }, ['--server'])).toThrow(/incomplete/)
    expect(() => readConfig({}, args)).toThrow('set POS_DB_PASSWORD')
  })
})

describe('tools', () => {
  it('sends search words as escaped parameters, never in the SQL text', async () => {
    const { db, calls } = fakeDb()
    await new PosTools(db, () => sunday).searchProducts({ query: `melk 50%'; DROP TABLE Stock--` })
    const search = calls.at(-1)!
    expect(search.sql).not.toContain('DROP')
    expect(search.sql).not.toContain('melk')
    expect(param(search.params, 'w0')).toBe('%melk%')
    expect(param(search.params, 'w1')).toBe('%50\\%\';%')
    expect(param(search.params, 'q')).toBe(`melk 50%'; DROP TABLE Stock--`)
  })

  it('quotes selling prices with VAT and reads whether costs include VAT from the POS settings', async () => {
    const product = { id: 1, name: 'Broodjies', price: 4.34783, cost: 5.75, vat: 15, on_hand: 12 }
    for (const [setting, costExcl] of [['false', 5.75], ['true', 5]] as const) {
      const { db } = fakeDb(sql => sql.includes('CostIncludesTax') ? [{ value: setting }] : [product])
      const [found] = (await new PosTools(db, () => sunday).searchProducts({ query: 'brood' })).products
      expect(found).toMatchObject({ price_incl_vat: 5, price_excl_vat: 4.35, cost_excl_vat: costExcl })
    }
  })

  it('reads the VAT setting at most every five minutes', async () => {
    let now = sunday
    const { db, calls } = fakeDb(sql => sql.includes('CostIncludesTax') ? [{ value: 'false' }] : [])
    const tools = new PosTools(db, () => now)
    await tools.costIncludesVat()
    await tools.costIncludesVat()
    now = new Date(sunday.getTime() + 6 * 60_000)
    await tools.costIncludesVat()
    expect(calls.filter(c => c.sql.includes('CostIncludesTax'))).toHaveLength(2)
  })

  it('counts only paid and account sales, in the requested local-day range', async () => {
    const { db, calls } = fakeDb(() => [{ sales: 4, revenue_incl_vat: 100 }])
    const result = await new PosTools(db, () => sunday).salesSummary({ period: 'last_7_days' })
    expect(result).toMatchObject({ period: { from: '2026-10-26', to: '2026-11-01' }, today: '2026-11-01', sales: 4, revenue_incl_vat: 100, average_sale_incl_vat: 25 })
    expect(calls[0]!.sql).toContain('m.SaleStatus IN (4, 9)')
    expect(param(calls[0]!.params, 'from')).toBe('20261026')
    expect(param(calls[0]!.params, 'to')).toBe('20261101')
  })

  it('caps row limits', async () => {
    const { db, calls } = fakeDb()
    await new PosTools(db, () => sunday).topProducts({ limit: 10_000 })
    expect(param(calls[0]!.params, 'limit')).toBe(50)
  })

  it('flags low stock and splits it by location', async () => {
    const { db } = fakeDb(sql => sql.includes('Locations')
      ? [{ id: 7, location: 'Default Location', on_hand: 2 }, { id: 7, location: 'Kombuis', on_hand: 1 }]
      : [{ id: 7, name: 'Kaas', on_hand: 3, warn_level: 5, reorder_qty: 10, order_pack: 2 }])
    const [row] = (await new PosTools(db, () => sunday).stockLevels({ low_only: true })).products
    expect(row).toMatchObject({ on_hand: 3, low: true, by_location: { 'Default Location': 2, 'Kombuis': 1 } })
  })

  it('refuses an unbounded stock listing', async () => {
    const { db } = fakeDb()
    await expect(new PosTools(db).stockLevels({})).rejects.toThrow(/give a search/)
  })

  it('estimates days of stock left from recent sales', async () => {
    const { db } = fakeDb(sql => sql.includes('SaleDetail')
      ? [{ day: '2026-10-31', quantity: 10 }, { day: '2026-11-01', quantity: 4 }]
      : [{ id: 9, name: 'Melk 2L', on_hand: 21 }])
    const result = await new PosTools(db, () => sunday).productSalesHistory({ product_id: 9, days: 7 })
    expect(result).toMatchObject({ period: { from: '2026-10-26', to: '2026-11-01' }, quantity_sold: 14, average_per_day: 2, days_of_stock_left: 10 })
  })

  it('names supplier order statuses and filters by them', async () => {
    const { db, calls } = fakeDb(() => [{ id: 1, status: 1 }, { id: 2, status: 3 }, { id: 3, status: 7 }])
    const { orders } = await new PosTools(db, () => sunday).supplierOrders({ status: 'received' })
    expect(orders.map(o => o.status)).toEqual(['received', 'cancelled', 'unknown (7)'])
    expect(param(calls[0]!.params, 'status')).toBe(1)
  })

  it('reports profit excluding VAT from the cost at the time of sale', async () => {
    const line = { sales: 10, revenue_incl_vat: 1150, revenue_excl_vat: 1000, cost_excl_vat: 700, lines_without_cost: 1, revenue_without_cost: 20 }
    const { db, calls } = fakeDb(sql => sql.includes('CostIncludesTax') ? [{ value: 'true' }] : [line])
    const report = await new PosTools(db, () => sunday).salesReport({ period: 'this_week', compare: true })
    expect(report).toMatchObject({
      currency: expect.stringContaining('rand'),
      period: { from: '2026-10-26', to: '2026-11-01' }, sales: 10, revenue_incl_vat: 1150, revenue_excl_vat: 1000, vat: 150,
      cost_excl_vat: 700, gross_profit_excl_vat: 300, margin_percent: 30, lines_without_cost: 1, revenue_without_cost_excl_vat: 20,
      previous: { period: { from: '2026-10-19', to: '2026-10-25' } }, change_percent: { revenue: 0, gross_profit: 0 },
    })
    const report_calls = calls.filter(c => c.sql.includes('SaleDetail'))
    expect(report_calls).toHaveLength(2)
    expect(report_calls[0]!.sql).toContain('m.SaleStatus IN (4, 9)')
    expect(param(report_calls[0]!.params, 'costIncl')).toBe(true)
  })

  it('breaks the report down by product, capped', async () => {
    const { db, calls } = fakeDb(sql => sql.includes('CostIncludesTax') ? [{ value: 'false' }]
      : sql.includes('GROUP BY') ? [{ name: 'Melk 2L', quantity: 5, revenue_incl_vat: 115, revenue_excl_vat: 100, cost_excl_vat: 80 }] : [{}])
    const report = await new PosTools(db, () => sunday).salesReport({ group_by: 'product', limit: 500 })
    expect(report.by_product).toEqual([{ name: 'Melk 2L', quantity: 5, revenue_incl_vat: 115, revenue_excl_vat: 100, vat: 15, cost_excl_vat: 80, gross_profit_excl_vat: 20, margin_percent: 20 }])
    expect(report.previous).toBeUndefined()
    const grouped = calls.find(c => c.sql.includes('GROUP BY'))!
    expect(param(grouped.params, 'limit')).toBe(100)
    expect(param(grouped.params, 'costIncl')).toBe(false)
  })
})
