# Weather contract

Status: agreed for the demo · protocol revision `2025-11-25` · conventions in [tool contracts](README.md)

One MCP server that exposes a daily weather forecast for a location.

## Tools

| Tool | Recommended class | Purpose |
| --- | --- | --- |
| `get_forecast` | `read` | Daily forecast for one location over at most 7 days |

## `get_forecast`

`from` and `to` are inclusive calendar dates in the location's time zone, which the result reports. Temperatures are degrees Celsius and precipitation is millimetres per day, as stated in `units`. Every day in the period appears in `days`.

```json
{
  "name": "get_forecast",
  "title": "Weather forecast",
  "description": "Daily weather forecast for one location over an inclusive date range of at most 7 days.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "location": {
        "type": "string",
        "title": "Location",
        "description": "Location identifier, for example brighton-uk.",
        "pattern": "^[a-z0-9-]{1,64}$"
      },
      "from": {
        "type": "string",
        "format": "date",
        "pattern": "^\\d{4}-\\d{2}-\\d{2}$",
        "description": "First forecast day (YYYY-MM-DD), inclusive.",
        "title": "From"
      },
      "to": {
        "type": "string",
        "format": "date",
        "pattern": "^\\d{4}-\\d{2}-\\d{2}$",
        "description": "Last forecast day (YYYY-MM-DD), inclusive.",
        "title": "To"
      }
    },
    "required": [
      "location",
      "from",
      "to"
    ],
    "additionalProperties": false
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "location": {
        "type": "object",
        "properties": {
          "id": {
            "type": "string"
          },
          "name": {
            "type": "string"
          },
          "timezone": {
            "type": "string",
            "description": "IANA time zone of the daily values."
          }
        },
        "required": [
          "id",
          "name",
          "timezone"
        ]
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
      "units": {
        "type": "object",
        "properties": {
          "temperature": {
            "type": "string",
            "const": "celsius"
          },
          "precipitation": {
            "type": "string",
            "const": "mm"
          }
        },
        "required": [
          "temperature",
          "precipitation"
        ]
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
            "temperature_max": {
              "type": "number"
            },
            "temperature_min": {
              "type": "number"
            },
            "precipitation": {
              "type": "number",
              "minimum": 0
            },
            "summary": {
              "type": "string"
            }
          },
          "required": [
            "date",
            "temperature_max",
            "temperature_min",
            "precipitation",
            "summary"
          ]
        }
      }
    },
    "required": [
      "location",
      "from",
      "to",
      "units",
      "days"
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
| `unknown_location` | No location has this identifier. |
| `invalid_period` | `to` is before `from`, or the period is longer than 7 days. |
| `no_forecast` | No forecast covers the whole period, for example dates beyond the forecast horizon. A partial period is an error, not a shorter list. |

### Example

Arguments:

```json
{
  "location": "brighton-uk",
  "from": "2026-07-13",
  "to": "2026-07-15"
}
```

Result:

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"location\":{\"id\":\"brighton-uk\",\"name\":\"Brighton, United Kingdom\",\"timezone\":\"Europe/London\"},\"from\":\"2026-07-13\",\"to\":\"2026-07-15\",\"units\":{\"temperature\":\"celsius\",\"precipitation\":\"mm\"},\"days\":[{\"date\":\"2026-07-13\",\"temperature_max\":29.5,\"temperature_min\":17.0,\"precipitation\":0.0,\"summary\":\"Sunny\"},{\"date\":\"2026-07-14\",\"temperature_max\":31.0,\"temperature_min\":18.5,\"precipitation\":0.0,\"summary\":\"Hot and sunny\"},{\"date\":\"2026-07-15\",\"temperature_max\":28.0,\"temperature_min\":17.5,\"precipitation\":0.4,\"summary\":\"Mostly sunny\"}]}"
    }
  ],
  "structuredContent": {
    "location": {
      "id": "brighton-uk",
      "name": "Brighton, United Kingdom",
      "timezone": "Europe/London"
    },
    "from": "2026-07-13",
    "to": "2026-07-15",
    "units": {
      "temperature": "celsius",
      "precipitation": "mm"
    },
    "days": [
      {
        "date": "2026-07-13",
        "temperature_max": 29.5,
        "temperature_min": 17.0,
        "precipitation": 0.0,
        "summary": "Sunny"
      },
      {
        "date": "2026-07-14",
        "temperature_max": 31.0,
        "temperature_min": 18.5,
        "precipitation": 0.0,
        "summary": "Hot and sunny"
      },
      {
        "date": "2026-07-15",
        "temperature_max": 28.0,
        "temperature_min": 17.5,
        "precipitation": 0.4,
        "summary": "Mostly sunny"
      }
    ]
  },
  "isError": false
}
```

### Error result example

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"error\":{\"code\":\"no_forecast\",\"message\":\"No forecast for brighton-uk from 2026-08-01 to 2026-08-03.\"}}"
    }
  ],
  "isError": true
}
```
