import type { Db, Param } from './db.js'
import { MAX_ROWS } from './db.js'
import { previousRange, resolveRange, sqlDate, type DateRange, type PeriodName } from './periods.js'

/**
 * RedTill facts the queries rely on:
 * - `SaleMaster.SaleStatus` 9 is Paid and 4 is AccountSale; only these are sales. Quotes, open
 *   sales, jobs, bookings, voided and cancelled sales are left out.
 * - `SaleMaster.SaleTotal` and `SaleDetail.TotalPriceInc` include VAT; `Stock.Price` excludes it.
 * - `ConfigTable.CostIncludesTax` says whether `Stock.UnitCost` includes VAT.
 * - `SupplierOrder.Status`: 0 Created, 1 Received, 2 Completed, 3 Cancelled.
 */
const SALE = 'm.SaleStatus IN (4, 9)'
const IN_RANGE = `${SALE} AND m.DateCompleted >= CAST(@from AS datetime) AND m.DateCompleted < DATEADD(day, 1, CAST(@to AS datetime))`
const ORDER_STATUS = ['created', 'received', 'completed', 'cancelled'] as const

/** Named in every result with money, because small models otherwise guess dollars or euros. */
const CURRENCY = 'ZAR: South African rand, written R'

/** A sale line's cost excluding VAT. `SaleDetail.UnitCost` is the cost when the sale was made;
 * it includes VAT when `@costIncl` (the POS's `CostIncludesTax` setting) is 1. */
const LINE_COST = 'd.Qty * ISNULL(d.UnitCost, 0) / CASE WHEN @costIncl = 1 THEN 1 + ISNULL(d.TaxPercentage, 0) / 100.0 ELSE 1 END'
const NO_COST = 'ISNULL(d.UnitCost, 0) <= 0 AND d.TotalPriceExcl > 0'
const REPORT_COLUMNS = `SUM(d.TotalPriceInc) AS revenue_incl_vat, SUM(d.TotalPriceExcl) AS revenue_excl_vat, SUM(${LINE_COST}) AS cost_excl_vat`
const REPORT_GROUPS = {
  day: { key: 'CONVERT(char(10), CAST(m.DateCompleted AS date), 120)', join: '', order: '1' },
  department: { key: 'ISNULL(dp.Name, \'(none)\')', join: 'LEFT JOIN Departments dp ON dp.DepartmentID = s.DepartmentID', order: 'SUM(d.TotalPriceExcl) DESC' },
  category: { key: 'ISNULL(c.Name, \'(none)\')', join: 'LEFT JOIN StockCategories c ON c.CategoryID = s.ItemCategory', order: 'SUM(d.TotalPriceExcl) DESC' },
  product: { key: 'ISNULL(s.Description, \'(unknown product)\')', join: '', order: 'SUM(d.TotalPriceExcl) DESC' },
} as const

/** How long the VAT setting is trusted before it is read again. */
const SETTINGS_TTL_MS = 5 * 60_000

/** Rounds money to cents and quantities to three decimals; leaves null as null. */
const money = (value: unknown) => value == null ? null : Math.round(Number(value) * 100) / 100
const qty = (value: unknown) => value == null ? null : Math.round(Number(value) * 1000) / 1000
const text = (value: unknown) => typeof value === 'string' ? value.trim() || null : value == null ? null : String(value)

const clamp = (value: number | undefined, fallback: number, max = MAX_ROWS) =>
  Math.max(1, Math.min(max, Math.floor(value ?? fallback)))

/** `%word%` with LIKE wildcards in the word escaped (`ESCAPE '\'`). */
function contains(word: string) {
  return `%${word.replace(/[\\%_[]/g, ch => `\\${ch}`)}%`
}

/** Up to four search words, each of which must appear in `column`. */
function wordFilter(query: string, column: string, prefix: string) {
  const words = query.trim().split(/\s+/).filter(Boolean).slice(0, 4)
  return {
    sql: words.map((_, i) => `${column} LIKE @${prefix}${i} ESCAPE '\\'`).join(' AND ') || '1 = 1',
    params: words.map((word, i): Param => ({ name: `${prefix}${i}`, type: 'text', value: contains(word) })),
  }
}

const rangeParams = (range: DateRange): Param[] => [
  { name: 'from', type: 'text', value: sqlDate(range.from) },
  { name: 'to', type: 'text', value: sqlDate(range.to) },
]

export interface PeriodArgs { period?: PeriodName, from?: string, to?: string }

export class PosTools {
  private costIncludesTax?: { value: boolean, at: number }

  constructor(private readonly db: Db, private readonly now: () => Date = () => new Date()) {}

  /** `ConfigTable.CostIncludesTax`, read at most every five minutes. */
  async costIncludesVat(): Promise<boolean> {
    const cached = this.costIncludesTax
    if (cached && this.now().getTime() - cached.at < SETTINGS_TTL_MS)
      return cached.value
    const rows = await this.db.query<{ value: string | null }>(
      `SELECT TOP 1 CAST(ConfigValue AS varchar(20)) AS value FROM ConfigTable WHERE ConfigItem = 'CostIncludesTax'`)
    const value = /^(true|1|yes)$/i.test(rows[0]?.value?.trim() ?? '')
    this.costIncludesTax = { value, at: this.now().getTime() }
    return value
  }

  /** Products whose name contains every word of `query`, or whose code or barcode is `query`. */
  async searchProducts(args: { query: string, limit?: number }) {
    const words = wordFilter(args.query, 's.Description', 'w')
    const costIncl = await this.costIncludesVat()
    const rows = await this.db.query<Record<string, unknown>>(`
      SELECT TOP (@limit) s.StockID AS id, s.ItemCode AS code, s.Barcode AS barcode, s.Description AS name,
        d.Name AS department, c.Name AS category, sp.Name AS supplier,
        CAST(s.Price AS float) AS price, CAST(s.UnitCost AS float) AS cost, ISNULL(t.Tax, 0) AS vat,
        (SELECT SUM(h.OnHand) FROM StockOnHand h WHERE h.StockID = s.StockID) AS on_hand
      FROM Stock s
      LEFT JOIN Departments d ON d.DepartmentID = s.DepartmentID
      LEFT JOIN StockCategories c ON c.CategoryID = s.ItemCategory
      LEFT JOIN Suppliers sp ON sp.SupplierID = s.Supplier
      LEFT JOIN SalesTax t ON t.TaxID = s.TaxID
      WHERE s.Deleted = 0 AND ((${words.sql}) OR s.ItemCode = @q OR s.Barcode = @q)
      ORDER BY CASE WHEN s.ItemCode = @q OR s.Barcode = @q THEN 0 ELSE 1 END, s.ImportanceWeight DESC, s.Description`,
    [{ name: 'limit', type: 'int', value: clamp(args.limit, 20, 50) }, { name: 'q', type: 'text', value: args.query.trim() }, ...words.params])
    return { currency: CURRENCY, products: rows.map(row => ({
      id: row.id, code: text(row.code), barcode: text(row.barcode), name: text(row.name),
      department: text(row.department), category: text(row.category), supplier: text(row.supplier),
      ...prices(row, costIncl), on_hand: qty(row.on_hand ?? 0),
    })) }
  }

  /** Stock on hand, in total and by location, for products matching a search, or those at or
   * below their warning level. */
  async stockLevels(args: { query?: string, product_ids?: number[], low_only?: boolean, department?: string, limit?: number }) {
    const filters = ['s.Deleted = 0']
    const params: Param[] = [{ name: 'limit', type: 'int', value: clamp(args.limit, 25, 100) }]
    if (args.query) {
      const words = wordFilter(args.query, 's.Description', 'w')
      filters.push(`((${words.sql}) OR s.ItemCode = @q OR s.Barcode = @q)`)
      params.push({ name: 'q', type: 'text', value: args.query.trim() }, ...words.params)
    }
    const ids = (args.product_ids ?? []).filter(Number.isInteger).slice(0, 50)
    if (ids.length) {
      filters.push(`s.StockID IN (${ids.map((_, i) => `@id${i}`).join(', ')})`)
      ids.forEach((id, i) => params.push({ name: `id${i}`, type: 'int', value: id }))
    }
    if (args.department) {
      filters.push(`d.Name LIKE @dept ESCAPE '\\'`)
      params.push({ name: 'dept', type: 'text', value: contains(args.department.trim()) })
    }
    if (!args.query && !ids.length && !args.low_only && !args.department)
      throw new Error('give a search, product ids, a department or low_only')
    const rows = await this.db.query<Record<string, unknown>>(`
      SELECT TOP (@limit) s.StockID AS id, s.ItemCode AS code, s.Description AS name, d.Name AS department,
        sp.Name AS supplier, ISNULL(SUM(h.OnHand), 0) AS on_hand, s.WarnLevel AS warn_level,
        s.ReorderQty AS reorder_qty, s.OrderPack AS order_pack
      FROM Stock s
      LEFT JOIN StockOnHand h ON h.StockID = s.StockID
      LEFT JOIN Departments d ON d.DepartmentID = s.DepartmentID
      LEFT JOIN Suppliers sp ON sp.SupplierID = s.Supplier
      WHERE ${filters.join(' AND ')}
      GROUP BY s.StockID, s.ItemCode, s.Description, d.Name, sp.Name, s.WarnLevel, s.ReorderQty, s.OrderPack, s.ImportanceWeight
      ${args.low_only ? 'HAVING s.WarnLevel > 0 AND ISNULL(SUM(h.OnHand), 0) <= s.WarnLevel' : ''}
      ORDER BY ${args.low_only ? 'ISNULL(SUM(h.OnHand), 0) - s.WarnLevel, s.Description' : 's.ImportanceWeight DESC, s.Description'}`, params)
    const found = rows.map(row => Number(row.id))
    const byLocation = found.length
      ? await this.db.query<{ id: number, location: string, on_hand: number }>(`
          SELECT h.StockID AS id, l.Location AS location, h.OnHand AS on_hand
          FROM StockOnHand h JOIN Locations l ON l.LocationID = h.LocationID
          WHERE h.OnHand <> 0 AND h.StockID IN (${found.map((_, i) => `@s${i}`).join(', ')})`,
        found.map((id, i): Param => ({ name: `s${i}`, type: 'int', value: id })))
      : []
    return { products: rows.map(row => ({
      id: row.id, code: text(row.code), name: text(row.name), department: text(row.department), supplier: text(row.supplier),
      on_hand: qty(row.on_hand),
      by_location: Object.fromEntries(byLocation.filter(l => l.id === row.id).map(l => [text(l.location), qty(l.on_hand)])),
      warn_level: qty(row.warn_level), reorder_qty: qty(row.reorder_qty), order_pack: qty(row.order_pack),
      low: Number(row.warn_level) > 0 && Number(row.on_hand) <= Number(row.warn_level),
    })) }
  }

  /** Sales totals for a period, overall or by day, department or category. Amounts include VAT. */
  async salesSummary(args: PeriodArgs & { group_by?: 'total' | 'day' | 'department' | 'category' }) {
    const range = resolveRange(args, this.now())
    const params = rangeParams(range)
    const totals = await this.db.query<Record<string, unknown>>(`
      SELECT COUNT(*) AS sales, SUM(m.SaleTotal) AS revenue_incl_vat FROM SaleMaster m WHERE ${IN_RANGE}`, params)
    const total = totals[0] ?? {}
    const sales = Number(total.sales ?? 0)
    const summary = {
      currency: CURRENCY, period: range, today: isoDay(this.now()), sales, revenue_incl_vat: money(total.revenue_incl_vat ?? 0),
      average_sale_incl_vat: sales ? money(Number(total.revenue_incl_vat) / sales) : null,
    }
    const group = args.group_by ?? 'total'
    if (group === 'total')
      return summary
    if (group === 'day') {
      const rows = await this.db.query<Record<string, unknown>>(`
        SELECT CONVERT(char(10), CAST(m.DateCompleted AS date), 120) AS day, COUNT(*) AS sales, SUM(m.SaleTotal) AS revenue_incl_vat
        FROM SaleMaster m WHERE ${IN_RANGE}
        GROUP BY CAST(m.DateCompleted AS date) ORDER BY day`, params)
      return { ...summary, by_day: rows.map(row => ({ day: row.day, sales: row.sales, revenue_incl_vat: money(row.revenue_incl_vat) })) }
    }
    const [key, join] = group === 'department'
      ? ['ISNULL(dp.Name, \'(none)\')', 'LEFT JOIN Departments dp ON dp.DepartmentID = s.DepartmentID']
      : ['ISNULL(c.Name, \'(none)\')', 'LEFT JOIN StockCategories c ON c.CategoryID = s.ItemCategory']
    const rows = await this.db.query<Record<string, unknown>>(`
      SELECT TOP (${MAX_ROWS}) ${key} AS name, SUM(d.Qty) AS quantity, SUM(d.TotalPriceInc) AS revenue_incl_vat
      FROM SaleMaster m JOIN SaleDetail d ON d.SaleID = m.SaleID
      LEFT JOIN Stock s ON s.StockID = d.StockID ${join}
      WHERE ${IN_RANGE}
      GROUP BY ${key} ORDER BY SUM(d.TotalPriceInc) DESC`, params)
    return { ...summary, [`by_${group}`]: rows.map(row => ({ name: text(row.name), quantity: qty(row.quantity), revenue_incl_vat: money(row.revenue_incl_vat) })) }
  }

  /** Revenue, VAT, cost of sales, gross profit and margin for a period, optionally broken down
   * and compared with the previous period. Cost is what each line cost when it was sold. */
  async salesReport(args: PeriodArgs & { group_by?: 'total' | keyof typeof REPORT_GROUPS, compare?: boolean, limit?: number }) {
    const range = resolveRange(args, this.now())
    const costIncl: Param = { name: 'costIncl', type: 'bit', value: await this.costIncludesVat() }
    const totals = async (r: DateRange) => {
      const row = (await this.db.query<Record<string, unknown>>(`
        SELECT COUNT(DISTINCT m.SaleID) AS sales, ${REPORT_COLUMNS},
          SUM(CASE WHEN ${NO_COST} THEN 1 ELSE 0 END) AS lines_without_cost,
          SUM(CASE WHEN ${NO_COST} THEN d.TotalPriceExcl ELSE 0 END) AS revenue_without_cost
        FROM SaleMaster m JOIN SaleDetail d ON d.SaleID = m.SaleID
        WHERE ${IN_RANGE}`, [...rangeParams(r), costIncl]))[0] ?? {}
      return {
        period: r, sales: Number(row.sales ?? 0), ...profit(row),
        lines_without_cost: Number(row.lines_without_cost ?? 0), revenue_without_cost_excl_vat: money(row.revenue_without_cost ?? 0),
      }
    }
    const current = await totals(range)
    const report: Record<string, unknown> = { currency: CURRENCY, ...current, today: isoDay(this.now()) }
    if (args.compare) {
      const previous = await totals(previousRange(range, args.from || args.to ? undefined : args.period ?? 'today'))
      report.previous = previous
      report.change_percent = {
        revenue: change(current.revenue_excl_vat, previous.revenue_excl_vat),
        gross_profit: change(current.gross_profit_excl_vat, previous.gross_profit_excl_vat),
        sales: change(current.sales, previous.sales),
      }
    }
    const group = args.group_by ?? 'total'
    if (group !== 'total') {
      const { key, join, order } = REPORT_GROUPS[group]
      const rows = await this.db.query<Record<string, unknown>>(`
        SELECT TOP (@limit) ${key} AS name, SUM(d.Qty) AS quantity, ${REPORT_COLUMNS}
        FROM SaleMaster m JOIN SaleDetail d ON d.SaleID = m.SaleID
        LEFT JOIN Stock s ON s.StockID = d.StockID ${join}
        WHERE ${IN_RANGE}
        GROUP BY ${key} ORDER BY ${order}`,
      [...rangeParams(range), costIncl, { name: 'limit', type: 'int', value: group === 'day' ? clamp(args.limit, MAX_ROWS) : clamp(args.limit, 25, 100) }])
      report[`by_${group}`] = rows.map(row => ({ name: text(row.name), quantity: qty(row.quantity), ...profit(row) }))
    }
    return report
  }

  /** Best-selling products for a period, by revenue or quantity. */
  async topProducts(args: PeriodArgs & { by?: 'revenue' | 'quantity', limit?: number, department?: string }) {
    const range = resolveRange(args, this.now())
    const params = [...rangeParams(range), { name: 'limit', type: 'int', value: clamp(args.limit, 10, 50) } as Param]
    let department = ''
    if (args.department) {
      department = `AND dp.Name LIKE @dept ESCAPE '\\'`
      params.push({ name: 'dept', type: 'text', value: contains(args.department.trim()) })
    }
    const order = args.by === 'quantity' ? 'SUM(d.Qty)' : 'SUM(d.TotalPriceInc)'
    const rows = await this.db.query<Record<string, unknown>>(`
      SELECT TOP (@limit) s.StockID AS id, s.Description AS name, dp.Name AS department,
        SUM(d.Qty) AS quantity, SUM(d.TotalPriceInc) AS revenue_incl_vat
      FROM SaleMaster m JOIN SaleDetail d ON d.SaleID = m.SaleID
      JOIN Stock s ON s.StockID = d.StockID
      LEFT JOIN Departments dp ON dp.DepartmentID = s.DepartmentID
      WHERE ${IN_RANGE} ${department}
      GROUP BY s.StockID, s.Description, dp.Name ORDER BY ${order} DESC`, params)
    return { currency: CURRENCY, period: range, today: isoDay(this.now()), by: args.by ?? 'revenue', products: rows.map(row => ({
      id: row.id, name: text(row.name), department: text(row.department), quantity: qty(row.quantity), revenue_incl_vat: money(row.revenue_incl_vat),
    })) }
  }

  /** Daily sales of one product, with stock on hand and how many days it should last. */
  async productSalesHistory(args: { product_id: number, days?: number }) {
    const days = clamp(args.days, 28, 366)
    const today = this.now()
    const range = resolveRange({ from: isoDay(new Date(today.getFullYear(), today.getMonth(), today.getDate() - days + 1)) }, today)
    const id: Param = { name: 'id', type: 'int', value: Math.floor(args.product_id) }
    const product = (await this.db.query<Record<string, unknown>>(`
      SELECT s.StockID AS id, s.Description AS name, s.WarnLevel AS warn_level, s.ReorderQty AS reorder_qty, s.OrderPack AS order_pack,
        sp.Name AS supplier, (SELECT SUM(h.OnHand) FROM StockOnHand h WHERE h.StockID = s.StockID) AS on_hand
      FROM Stock s LEFT JOIN Suppliers sp ON sp.SupplierID = s.Supplier WHERE s.StockID = @id`, [id]))[0]
    if (!product)
      throw new Error(`no product with id ${args.product_id}`)
    const rows = await this.db.query<Record<string, unknown>>(`
      SELECT CONVERT(char(10), CAST(m.DateCompleted AS date), 120) AS day, SUM(d.Qty) AS quantity, SUM(d.TotalPriceInc) AS revenue_incl_vat
      FROM SaleDetail d JOIN SaleMaster m ON m.SaleID = d.SaleID
      WHERE d.StockID = @id AND ${IN_RANGE}
      GROUP BY CAST(m.DateCompleted AS date) ORDER BY day`, [id, ...rangeParams(range)])
    const sold = rows.reduce((sum, row) => sum + Number(row.quantity ?? 0), 0)
    const perDay = sold / days
    const onHand = Number(product.on_hand ?? 0)
    return {
      product: { id: product.id, name: text(product.name), supplier: text(product.supplier), on_hand: qty(onHand),
        warn_level: qty(product.warn_level), reorder_qty: qty(product.reorder_qty), order_pack: qty(product.order_pack) },
      period: range, today: isoDay(today), quantity_sold: qty(sold), average_per_day: qty(perDay),
      days_of_stock_left: perDay > 0 ? Math.floor(Math.max(0, onHand) / perDay) : null,
      by_day: rows.map(row => ({ day: row.day, quantity: qty(row.quantity), revenue_incl_vat: money(row.revenue_incl_vat) })),
    }
  }

  /** Active suppliers, optionally matching a name. */
  async listSuppliers(args: { query?: string, limit?: number }) {
    const words = wordFilter(args.query ?? '', 'sp.Name', 'w')
    const rows = await this.db.query<Record<string, unknown>>(`
      SELECT TOP (@limit) sp.SupplierID AS id, sp.SupplierCode AS code, sp.Name AS name,
        (SELECT COUNT(*) FROM Stock s WHERE s.Supplier = sp.SupplierID AND s.Deleted = 0) AS products
      FROM Suppliers sp WHERE sp.Deleted = 0 AND ${words.sql} ORDER BY sp.Name`,
    [{ name: 'limit', type: 'int', value: clamp(args.limit, 50, 150) }, ...words.params])
    return { suppliers: rows.map(row => ({ id: row.id, code: text(row.code), name: text(row.name), products: row.products })) }
  }

  /** Recent supplier orders with their status and line totals. */
  async supplierOrders(args: { supplier_id?: number, status?: typeof ORDER_STATUS[number], days?: number, limit?: number }) {
    const filters = ['o.OrderCreated >= DATEADD(day, -@days, CAST(GETDATE() AS date))']
    const params: Param[] = [
      { name: 'days', type: 'int', value: clamp(args.days, 90, 366) },
      { name: 'limit', type: 'int', value: clamp(args.limit, 20, 100) },
    ]
    if (args.supplier_id != null) {
      filters.push('o.SupplierID = @supplier')
      params.push({ name: 'supplier', type: 'int', value: Math.floor(args.supplier_id) })
    }
    if (args.status) {
      filters.push('o.Status = @status')
      params.push({ name: 'status', type: 'int', value: ORDER_STATUS.indexOf(args.status) })
    }
    const rows = await this.db.query<Record<string, unknown>>(`
      SELECT TOP (@limit) o.OrderID AS id, sp.Name AS supplier, o.Status AS status,
        CONVERT(char(16), o.OrderCreated, 120) AS created, CONVERT(char(16), o.OrderReceived, 120) AS received,
        COUNT(od.OrderDetailID) AS lines, SUM(od.QtyOrdered) AS quantity_ordered, SUM(od.QtyReceived) AS quantity_received
      FROM SupplierOrder o
      LEFT JOIN Suppliers sp ON sp.SupplierID = o.SupplierID
      LEFT JOIN SupplierOrderDetail od ON od.OrderID = o.OrderID
      WHERE ${filters.join(' AND ')}
      GROUP BY o.OrderID, sp.Name, o.Status, o.OrderCreated, o.OrderReceived
      ORDER BY o.OrderCreated DESC`, params)
    return { orders: rows.map(row => ({
      id: row.id, supplier: text(row.supplier), status: ORDER_STATUS[Number(row.status)] ?? `unknown (${String(row.status)})`,
      created: text(row.created), received: text(row.received), lines: row.lines,
      quantity_ordered: qty(row.quantity_ordered), quantity_received: qty(row.quantity_received),
    })) }
  }
}

function isoDay(date: Date) {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

/** Revenue with and without VAT, VAT, cost, gross profit and margin from summed sale lines. */
function profit(row: Record<string, unknown>) {
  const incl = Number(row.revenue_incl_vat ?? 0)
  const excl = Number(row.revenue_excl_vat ?? 0)
  const cost = Number(row.cost_excl_vat ?? 0)
  return {
    revenue_incl_vat: money(incl), revenue_excl_vat: money(excl), vat: money(incl - excl),
    cost_excl_vat: money(cost), gross_profit_excl_vat: money(excl - cost),
    margin_percent: excl > 0 ? Math.round((excl - cost) / excl * 1000) / 10 : null,
  }
}

/** Percentage change from `before` to `now`, to one decimal; null when there is nothing before. */
function change(now: number | null, before: number | null) {
  return now == null || !before ? null : Math.round((now - before) / Math.abs(before) * 1000) / 10
}

/** Selling price and cost, each excluding and including VAT. Selling prices are stored excluding
 * VAT; costs follow the POS's `CostIncludesTax` setting. */
function prices(row: Record<string, unknown>, costIncludesVat: boolean) {
  const rate = 1 + Number(row.vat ?? 0) / 100
  const price = Number(row.price ?? 0)
  const cost = Number(row.cost ?? 0)
  const costExcl = costIncludesVat ? cost / rate : cost
  return {
    price_incl_vat: money(price * rate), price_excl_vat: money(price),
    cost_excl_vat: money(costExcl), cost_incl_vat: money(costExcl * rate),
    margin_percent: price > 0 ? Math.round((price - costExcl) / price * 1000) / 10 : null,
  }
}
