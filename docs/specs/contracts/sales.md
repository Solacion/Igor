# Sales and stock contract

Status: agreed for the demo · protocol revision `2025-11-25` · conventions in [tool contracts](README.md)

One MCP server that exposes a supplier's stock levels and sales history, and saves draft purchase orders. Only the tools that the example request "Create an order for supplier X" needs are defined.

## Tools

| Tool | Recommended class | Purpose |
| --- | --- | --- |
| `get_stock_levels` | `read` | Stock per product of one supplier on a date |
| `get_sales_history` | `read` | Units sold per product per day over a period |
| `create_draft_order` | `write` | Save a draft order for review; nothing is submitted |

All quantities are in ordering units, for example cases. The operator supplies every date and period; a server never assumes "today".

## `get_stock_levels`

```json
{
  "name": "get_stock_levels",
  "title": "Stock levels",
  "description": "Stock on hand and on order for each product of one supplier, at the end of the given date.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "supplier_id": {
        "type": "string",
        "title": "Supplier",
        "description": "Supplier identifier, for example supplier-x.",
        "pattern": "^[a-z0-9-]{1,64}$"
      },
      "date": {
        "type": "string",
        "format": "date",
        "pattern": "^\\d{4}-\\d{2}-\\d{2}$",
        "description": "Stock snapshot date (YYYY-MM-DD).",
        "title": "Date"
      }
    },
    "required": [
      "supplier_id",
      "date"
    ],
    "additionalProperties": false
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "supplier": {
        "type": "object",
        "properties": {
          "id": {
            "type": "string"
          },
          "name": {
            "type": "string"
          }
        },
        "required": [
          "id",
          "name"
        ]
      },
      "date": {
        "type": "string",
        "format": "date",
        "pattern": "^\\d{4}-\\d{2}-\\d{2}$"
      },
      "products": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "sku": {
              "type": "string"
            },
            "name": {
              "type": "string"
            },
            "unit": {
              "type": "string",
              "description": "Ordering unit, for example case."
            },
            "pack_size": {
              "type": "integer",
              "minimum": 1,
              "description": "Items per ordering unit."
            },
            "on_hand": {
              "type": "integer",
              "minimum": 0,
              "description": "Ordering units on hand."
            },
            "on_order": {
              "type": "integer",
              "minimum": 0,
              "description": "Ordering units ordered but not yet delivered."
            },
            "reorder_point": {
              "type": "integer",
              "minimum": 0,
              "description": "Stock level, in ordering units, at or below which the product should be reordered."
            }
          },
          "required": [
            "sku",
            "name",
            "unit",
            "pack_size",
            "on_hand",
            "on_order",
            "reorder_point"
          ]
        }
      }
    },
    "required": [
      "supplier",
      "date",
      "products"
    ]
  },
  "annotations": {
    "readOnlyHint": true,
    "openWorldHint": false
  }
}
```

### Errors

| Code | When |
| --- | --- |
| `unknown_supplier` | No supplier has this `supplier_id`. |
| `no_data` | The supplier exists but no stock snapshot exists for `date`. |

### Example

Arguments:

```json
{
  "supplier_id": "supplier-x",
  "date": "2026-07-13"
}
```

Result, showing one of the four products the fixture returns:

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"supplier\":{\"id\":\"supplier-x\",\"name\":\"Supplier X Drinks Co.\"},\"date\":\"2026-07-13\",\"products\":[{\"sku\":\"COLA-330-24\",\"name\":\"Cola 330 ml can\",\"unit\":\"case\",\"pack_size\":24,\"on_hand\":18,\"on_order\":0,\"reorder_point\":20}]}"
    }
  ],
  "structuredContent": {
    "supplier": {
      "id": "supplier-x",
      "name": "Supplier X Drinks Co."
    },
    "date": "2026-07-13",
    "products": [
      {
        "sku": "COLA-330-24",
        "name": "Cola 330 ml can",
        "unit": "case",
        "pack_size": 24,
        "on_hand": 18,
        "on_order": 0,
        "reorder_point": 20
      }
    ]
  },
  "isError": false
}
```

## `get_sales_history`

`from` and `to` are inclusive. Every day in the period appears in `days`, with `units` of zero on days without sales.

```json
{
  "name": "get_sales_history",
  "title": "Sales history",
  "description": "Ordering units (for example cases) sold per day for each product of one supplier over an inclusive date range of at most 31 days.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "supplier_id": {
        "type": "string",
        "title": "Supplier",
        "description": "Supplier identifier, for example supplier-x.",
        "pattern": "^[a-z0-9-]{1,64}$"
      },
      "from": {
        "type": "string",
        "format": "date",
        "pattern": "^\\d{4}-\\d{2}-\\d{2}$",
        "description": "First day of the period (YYYY-MM-DD), inclusive.",
        "title": "From"
      },
      "to": {
        "type": "string",
        "format": "date",
        "pattern": "^\\d{4}-\\d{2}-\\d{2}$",
        "description": "Last day of the period (YYYY-MM-DD), inclusive.",
        "title": "To"
      }
    },
    "required": [
      "supplier_id",
      "from",
      "to"
    ],
    "additionalProperties": false
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "supplier_id": {
        "type": "string"
      },
      "from": {
        "type": "string",
        "format": "date",
        "pattern": "^\\d{4}-\\d{2}-\\d{2}$"
      },
      "to": {
        "type": "string",
        "format": "date",
        "pattern": "^\\d{4}-\\d{2}-\\d{2}$"
      },
      "products": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "sku": {
              "type": "string"
            },
            "total_units": {
              "type": "integer",
              "minimum": 0
            },
            "days": {
              "type": "array",
              "items": {
                "type": "object",
                "properties": {
                  "date": {
                    "type": "string",
                    "format": "date",
                    "pattern": "^\\d{4}-\\d{2}-\\d{2}$"
                  },
                  "units": {
                    "type": "integer",
                    "minimum": 0
                  }
                },
                "required": [
                  "date",
                  "units"
                ]
              }
            }
          },
          "required": [
            "sku",
            "total_units",
            "days"
          ]
        }
      }
    },
    "required": [
      "supplier_id",
      "from",
      "to",
      "products"
    ]
  },
  "annotations": {
    "readOnlyHint": true,
    "openWorldHint": false
  }
}
```

### Errors

| Code | When |
| --- | --- |
| `unknown_supplier` | No supplier has this `supplier_id`. |
| `invalid_period` | `to` is before `from`, or the period is longer than 31 days. |
| `no_data` | No sales data covers the whole period. A partial period is an error, not a shorter list. |

### Example

Arguments:

```json
{
  "supplier_id": "supplier-x",
  "from": "2026-07-06",
  "to": "2026-07-12"
}
```

Result, showing one of the four products the fixture returns:

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"supplier_id\":\"supplier-x\",\"from\":\"2026-07-06\",\"to\":\"2026-07-12\",\"products\":[{\"sku\":\"COLA-330-24\",\"total_units\":59,\"days\":[{\"date\":\"2026-07-06\",\"units\":6},{\"date\":\"2026-07-07\",\"units\":7},{\"date\":\"2026-07-08\",\"units\":6},{\"date\":\"2026-07-09\",\"units\":8},{\"date\":\"2026-07-10\",\"units\":9},{\"date\":\"2026-07-11\",\"units\":11},{\"date\":\"2026-07-12\",\"units\":12}]}]}"
    }
  ],
  "structuredContent": {
    "supplier_id": "supplier-x",
    "from": "2026-07-06",
    "to": "2026-07-12",
    "products": [
      {
        "sku": "COLA-330-24",
        "total_units": 59,
        "days": [
          {
            "date": "2026-07-06",
            "units": 6
          },
          {
            "date": "2026-07-07",
            "units": 7
          },
          {
            "date": "2026-07-08",
            "units": 6
          },
          {
            "date": "2026-07-09",
            "units": 8
          },
          {
            "date": "2026-07-10",
            "units": 9
          },
          {
            "date": "2026-07-11",
            "units": 11
          },
          {
            "date": "2026-07-12",
            "units": 12
          }
        ]
      }
    ]
  },
  "isError": false
}
```

## `create_draft_order`

The arguments are the draft order preview. The host shows them for approval before the call, rendering them generically from `inputSchema`: field `title` and `description` as labels, and arrays of objects as tables. No order-specific code is needed in the core or the UI.

The tool saves a draft only. It does not submit the order, reserve stock, take payment or change any external system. The result identifies the draft and repeats none of the preview.

```json
{
  "name": "create_draft_order",
  "title": "Create draft order",
  "description": "Saves a draft purchase order for review. The draft is not submitted to the supplier and no stock, payment or external system changes.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "supplier_id": {
        "type": "string",
        "title": "Supplier",
        "description": "Supplier identifier, for example supplier-x.",
        "pattern": "^[a-z0-9-]{1,64}$"
      },
      "delivery_date": {
        "type": "string",
        "format": "date",
        "pattern": "^\\d{4}-\\d{2}-\\d{2}$",
        "description": "Requested delivery date (YYYY-MM-DD).",
        "title": "Delivery date"
      },
      "lines": {
        "type": "array",
        "title": "Order lines",
        "minItems": 1,
        "maxItems": 50,
        "items": {
          "type": "object",
          "properties": {
            "sku": {
              "type": "string",
              "title": "SKU",
              "description": "Product SKU from get_stock_levels."
            },
            "quantity": {
              "type": "integer",
              "title": "Quantity",
              "description": "Number of ordering units (for example cases).",
              "minimum": 1,
              "maximum": 1000
            },
            "rationale": {
              "type": "string",
              "title": "Rationale",
              "description": "Why this quantity is proposed.",
              "maxLength": 500
            }
          },
          "required": [
            "sku",
            "quantity",
            "rationale"
          ],
          "additionalProperties": false
        }
      },
      "notes": {
        "type": "string",
        "title": "Notes",
        "description": "Optional notes for the reviewer.",
        "maxLength": 1000
      }
    },
    "required": [
      "supplier_id",
      "delivery_date",
      "lines"
    ],
    "additionalProperties": false
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "draft_id": {
        "type": "string"
      },
      "status": {
        "type": "string",
        "const": "draft"
      },
      "submitted": {
        "type": "boolean",
        "const": false
      }
    },
    "required": [
      "draft_id",
      "status",
      "submitted"
    ]
  },
  "annotations": {
    "readOnlyHint": false,
    "destructiveHint": false,
    "idempotentHint": false,
    "openWorldHint": false
  }
}
```

### Errors

| Code | When |
| --- | --- |
| `unknown_supplier` | No supplier has this `supplier_id`. |
| `unknown_sku` | A line's `sku` is not supplied by this supplier. The message names the SKU. |
| `invalid_quantity` | A quantity is not a whole number of ordering units within the schema bounds. |

A timeout, a disconnect after sending, or a malformed response leaves the outcome unknown. This is not a server result: the client reports it as an unknown outcome. The host must not retry the call automatically, because a draft may already exist.

### Example

Arguments:

```json
{
  "supplier_id": "supplier-x",
  "delivery_date": "2026-07-15",
  "lines": [
    {
      "sku": "WATER-500-24",
      "quantity": 20,
      "rationale": "Sales rising with hot weather; 22 on hand against 15 a day."
    }
  ],
  "notes": "Check delivery slot."
}
```

Result:

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"draft_id\":\"draft-0001\",\"status\":\"draft\",\"submitted\":false}"
    }
  ],
  "structuredContent": {
    "draft_id": "draft-0001",
    "status": "draft",
    "submitted": false
  },
  "isError": false
}
```

## Error result example

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"error\":{\"code\":\"unknown_supplier\",\"message\":\"No supplier with id supplier-unknown.\"}}"
    }
  ],
  "isError": true
}
```
