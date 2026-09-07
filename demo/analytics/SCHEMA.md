# events.jsonl schema (chosen by #6/analytics)

`demo/data/events.jsonl` is newline-delimited JSON. One object per line, one
line per website traffic event.

## Fields

| field         | type   | notes                                                              |
|---------------|--------|---------------------------------------------------------------------|
| `event_id`    | string | unique, format `evt-NNNN`                                          |
| `event_type`  | string | one of: `page_view`, `click`, `signup`, `purchase`                 |
| `timestamp`   | string | UTC, ISO-8601, format `YYYY-MM-DDTHH:MM:SSZ`                       |
| `user_id`     | string | format `user-NNN`, drawn from a small fixed pool of users          |
| `page`        | string | site path, e.g. `/home`, `/pricing`, `/checkout`                   |
| `value_cents` | number | integer cents; nonzero only when `event_type` is `purchase`, else 0 |

## Example line

```json
{"event_id":"evt-0001","event_type":"purchase","timestamp":"2026-09-05T00:00:17Z","user_id":"user-004","page":"/checkout","value_cents":4231}
```

## Consumer notes

- `value_cents` is always present (never null/omitted) so consumers can sum
  it unconditionally without a presence check.
- `event_type` is a closed set of exactly the four values above — no other
  values will appear.
