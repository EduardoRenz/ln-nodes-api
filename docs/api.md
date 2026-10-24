# HTTP API

- **Base URL (local):** `http://localhost:3000`
- **Content type:** `application/json`
- **Authentication:** none
- **Style:** read-only JSON over HTTP

---

## Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/nodes` | Return the stored node list |
| `GET` | `/health` | Report service and database health |

---

## `GET /nodes`

Returns the stored nodes as a JSON array.

### Request

```http
GET /nodes HTTP/1.1
Host: localhost:3000
Accept: application/json
```

No query parameters, body, or authentication are required.

### Response

**Status:** `200 OK`

```json
[
  {
    "public_key": "03864ef025fde8fb587d989186ce6a4a186895ee44a926bfc370e2c366597a3f8f",
    "alias": "ACINQ",
    "capacity": "360.10516297",
    "first_seen": "2018-04-05T15:13:42Z"
  },
  {
    "public_key": "035e4ff418fc8b5554c5d9eea66396c227bd429a3251c8cbc711002ba215bfc226",
    "alias": "WalletOfSatoshi.com",
    "capacity": "154.64503162",
    "first_seen": "2020-09-30T01:39:00Z"
  }
]
```

### Fields

| Field | Type | Description |
| --- | --- | --- |
| `public_key` | string | Node public key, exactly as imported. |
| `alias` | string | Node alias, exactly as imported. |
| `capacity` | string | Capacity in Bitcoin, with exactly 8 decimal places. |
| `first_seen` | string | First-seen timestamp in RFC 3339 UTC format. |

### Capacity format

`capacity` is a string, not a number, to preserve precision:

```text
1 BTC = 100,000,000 sats

36010516297 sats -> "360.10516297"
550000 sats      -> "0.00550000"
```

### Timestamp format

`first_seen` is formatted as RFC 3339 UTC with a `Z` suffix:

```text
1522941222 -> "2018-04-05T15:13:42Z"
```

### Ordering and empty results

- Results are ordered by the node's connectivity ranking.
- If there are no stored nodes, the response is `[]`.

### Errors

| Status | Body | When |
| --- | --- | --- |
| `429` | `{"error":"too many requests"}` | Too many requests. A `Retry-After` header gives the seconds to wait. |
| `500` | `{"error":"internal server error"}` | The node list cannot be read. |

---

## `GET /health`

Checks service and database availability.

### Success

**Status:** `200 OK`

```json
{ "status": "ok" }
```

### Failure

**Status:** `503 Service Unavailable`

```json
{ "status": "unavailable" }
```

---

## Errors

Errors use a JSON object with a single `error` field:

```json
{ "error": "internal server error" }
```

| Status | Meaning |
| --- | --- |
| `200` | Success |
| `404` | Unknown route |
| `429` | Too many requests |
| `500` | Internal error while reading nodes |
| `503` | Database unavailable |

Unknown routes return:

```json
{ "error": "not found" }
```

---

## Examples

Fetch all nodes:

```bash
curl -s http://localhost:3000/nodes
```

Pretty-print the response:

```bash
curl -s http://localhost:3000/nodes | jq
```

Check health:

```bash
curl -i http://localhost:3000/health
```

---

## Scope

The API is read-only.

It does not provide:

- `POST`, `PUT`, `PATCH`, or `DELETE` endpoints
- Pagination, filtering, or sorting parameters
- Authentication or authorization
- CORS
- Response caching or ETags
