# Supplier catalogue contract

Status: agreed for the demo · protocol revision `2025-11-25` · conventions in [tool contracts](README.md)

One MCP server that exposes a supplier's ordering terms and catalogue. Together with the [sales and stock](sales.md) server it lets the agent turn expected demand into a valid order: whole ordering units, the supplier's order multiples, its minimum order value and its delivery days.

## Tools

| Tool | Recommended class | Purpose |
| --- | --- | --- |
| `get_catalogue` | `read` | Ordering terms and products of one supplier |

SKUs match the [sales and stock](sales.md) contract. Prices are per ordering unit, in the reported `currency`.

## `get_catalogue`

A product with `available: false` is listed so the agent can explain why it is left out of an order; it cannot be ordered.

```json
{
  "name": "get_catalogue",
  "title": "Supplier catalogue",
  "description": "Ordering terms and the products one supplier sells: pack sizes, prices, order multiples, minimum order value, lead time and delivery days.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "supplier_id": {
        "type": "string",
        "title": "Supplier",
        "description": "Supplier identifier, for example supplier-x.",
        "pattern": "^[a-z0-9-]{1,64}$"
      }
    },
    "required": [
      "supplier_id"
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
      "currency": {
        "type": "string",
        "pattern": "^[A-Z]{3}$",
        "description": "ISO 4217 currency of every price."
      },
      "min_order_value": {
        "type": "number",
        "minimum": 0,
        "description": "Smallest order total the supplier accepts, in currency."
      },
      "lead_time_days": {
        "type": "integer",
        "minimum": 0,
        "description": "Days from order to the earliest delivery."
      },
      "delivery_days": {
        "type": "array",
        "items": {
          "type": "string",
          "enum": [
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday"
          ]
        },
        "description": "Weekdays the supplier delivers."
      },
      "items": {
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
            "unit_price": {
              "type": "number",
              "minimum": 0,
              "description": "Price per ordering unit, in currency."
            },
            "order_multiple": {
              "type": "integer",
              "minimum": 1,
              "description": "Quantities must be a multiple of this many ordering units."
            },
            "available": {
              "type": "boolean",
              "description": "False when the product cannot be ordered at the moment."
            }
          },
          "required": [
            "sku",
            "name",
            "unit",
            "pack_size",
            "unit_price",
            "order_multiple",
            "available"
          ]
        }
      }
    },
    "required": [
      "supplier",
      "currency",
      "min_order_value",
      "lead_time_days",
      "delivery_days",
      "items"
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

### Example

Arguments:

```json
{
  "supplier_id": "supplier-x"
}
```

Result, showing one of the four products the fixture returns:

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"supplier\":{\"id\":\"supplier-x\",\"name\":\"Supplier X Drinks Co.\"},\"currency\":\"ZAR\",\"min_order_value\":1500.0,\"lead_time_days\":2,\"delivery_days\":[\"monday\",\"wednesday\",\"friday\"],\"items\":[{\"sku\":\"COLA-330-24\",\"name\":\"Cola 330 ml can\",\"unit\":\"case\",\"pack_size\":24,\"unit_price\":189.0,\"order_multiple\":2,\"available\":true}]}"
    }
  ],
  "structuredContent": {
    "supplier": {
      "id": "supplier-x",
      "name": "Supplier X Drinks Co."
    },
    "currency": "ZAR",
    "min_order_value": 1500.0,
    "lead_time_days": 2,
    "delivery_days": [
      "monday",
      "wednesday",
      "friday"
    ],
    "items": [
      {
        "sku": "COLA-330-24",
        "name": "Cola 330 ml can",
        "unit": "case",
        "pack_size": 24,
        "unit_price": 189.0,
        "order_multiple": 2,
        "available": true
      }
    ]
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
