# summary.json shape (chosen by #6/backend)

`demo/data/summary.json` is a single JSON object aggregating all of
`demo/data/events.jsonl` (schema: `demo/analytics/SCHEMA.md`).

## Fields

| field            | type   | notes                                                              |
|------------------|--------|---------------------------------------------------------------------|
| `total_events`   | number | count of all event lines                                          |
| `unique_users`   | number | distinct `user_id` values                                         |
| `revenue_cents`  | number | sum of `value_cents` across all events                            |
| `by_event_type`  | object | keys `page_view`, `click`, `signup`, `purchase` -> count (always all four keys, 0 if absent) |
| `top_pages`      | array  | up to 5 objects `{"page": string, "count": number}`, sorted by `count` descending |
| `generated_at`   | string | UTC ISO-8601 timestamp of when aggregation ran                    |

## Example

```json
{
  "total_events": 40,
  "unique_users": 8,
  "revenue_cents": 92827,
  "by_event_type": {"page_view": 10, "click": 8, "signup": 10, "purchase": 12},
  "top_pages": [
    {"page": "/home", "count": 9},
    {"page": "/docs", "count": 6}
  ],
  "generated_at": "2026-09-05T13:45:00Z"
}
```

## Consumer notes

- `by_event_type` always has all four keys present (never omitted), so
  consumers can index them unconditionally.
- `top_pages` may have fewer than 5 entries if fewer distinct pages exist.
