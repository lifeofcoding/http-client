# http-client

A Rust CLI debugging tool that executes `.http` files (REST Client / JetBrains HTTP format) against real or local endpoints and prints response information. It ships with an embedded echo server so you can test requests entirely offline — including with injected secrets.

```
→ POST http://127.0.0.1:8080/api/cleanupVoices
← 200 OK · 738µs
{
  "body": "...",
  "bodyLength": 14,
  ...
}
```

Output is ANSI-colored when stdout is a terminal; set `NO_COLOR` or pipe output to disable.

## Build

Requires Rust (via [rustup](https://rustup.rs)).

```sh
cargo build --release     # binary at ./target/release/http-client
```

## Usage

```
http-client <COMMAND>
```

| Command | Description |
|---|---|
| `run <FILE>` | Execute the requests in a `.http` file |
| `serve` | Start the built-in echo server |
| `demo` | Spawn an ephemeral echo server and fire sample requests at it (offline self-test) |

### `http-client run`

```sh
http-client run requests.http [OPTIONS]
```

| Option | Description |
|---|---|
| `-r, --request <N>` | Run only request `N` (1-based) |
| `--dry-run` | Print the parsed request(s) without sending anything |
| `-v, --verbose` | Also print response headers |
| `--no-pretty` | Don't pretty-print JSON response bodies |
| `--env-file <FILE>` | Load a `KEY=value` file as a secret source (repeatable; see below) |
| `--no-redact` | Disable redaction of substituted secret values in output |

Exit code is `0` when every request succeeded, `1` otherwise (including parse errors, missing files, and unresolved variables).

### `http-client serve`

```sh
http-client serve [--port 8080] [--status 200]
```

Echo server that accepts any method and path and responds with JSON mirroring the received request:

```json
{
  "method": "POST",
  "path": "/login",
  "headers": [{ "name": "authorization", "value": "Bearer ..." }],
  "bodyLength": 14,
  "body": "..."
}
```

Useful for developing and testing `.http` files without hitting real APIs.

## The `.http` file format

```http
# Requests are separated by lines starting with ###
# Lines starting with # are comments

GET http://127.0.0.1:8080/health HTTP/1.1
Accept: application/json

###

POST http://127.0.0.1:8080/api/cleanupVoices HTTP/1.1
content-type: application/json
Authorization: Bearer demo-token

{
  "limit": 300
}
```

Rules:

- First meaningful line of each request: `METHOD URL [HTTP/x.x]` — the HTTP version token is accepted and ignored
- Header lines (`Name: value`) until the first blank line
- Everything after the blank line is the body (trimmed); omitted if empty

## Secret injection

Reference secrets with `{{VAR}}` placeholders — in URLs, header names, header values, and bodies:

```http
### login with injected secrets
POST https://api.example.com/login
Authorization: Bearer {{API_TOKEN}}
X-Client: {{CLIENT_NAME}}

{"user": "{{USER_ID}}"}
```

### Secret sources

1. **Process environment variables** — any `{{NAME}}` can resolve to any env var
2. **`--env-file <FILE>`** (repeatable) — dotenv-style `KEY=value` files

File entries **override** process env vars with the same name. `.env` files are not auto-detected; always pass `--env-file` explicitly.

```sh
http-client run requests.http --env-file .env
```

`.env` format:

```sh
# comments and blank lines are skipped
API_TOKEN=super-secret-token-123
CLIENT_NAME=http-client-tests
USER_ID=42
EMPTY=
QUOTED="quotes are stripped"
export EXPORT_PREFIX=also-supported
VALUE_WITH_EQUALS=a=b=c
```

- Keys must be `[A-Za-z0-9_]+`; anything else is a load error
- Values may contain `=`
- Surrounding single or double quotes are stripped

### Fail closed on missing variables

Unresolved or malformed placeholders abort the run **before any request is sent** — a literal placeholder is never sent to a server. All problems are reported together:

```
error: variable substitution failed in requests.http:
  MISSING_TOKEN (requests 1, 3)
  unterminated {{ placeholder (request 2)
```

Because of this, bodies that legitimately contain literal `{{` (e.g. Mustache templates) will error — there is no escape syntax.

`--request N` only substitutes the selected request(s), so unresolved variables in other requests don't block a single-request run:

```sh
http-client run requests.http --env-file .env --request 2
```

### Redaction

Substituted values are masked as `[redacted]` everywhere they could appear in console output — the `→` request line, `--dry-run` output, `--verbose` response headers, and rendered response bodies (relevant when the echo server reflects your `Authorization` header back):

```
→ POST http://127.0.0.1:8080/login
  "body": "{\"user\": \"[redacted]\"}",
      "value": "Bearer [redacted]"
```

Disable with `--no-redact` when you want to see resolved values for debugging.

## End-to-end example

```sh
# terminal 1
http-client serve

# terminal 2
http-client run example.http                      # fails closed: TOKEN (request 2)
TOKEN=demo-token http-client run example.http     # resolved, output shows [redacted]
http-client run example.http --no-redact          # resolved, values visible
```

`demo` (below) exercises substitution and redaction without any editing or setup.

`demo` spawns an ephemeral echo server and fires three sample requests at it, including one that injects `{{DEMO_TOKEN}}` (defaults to `demo-token`, override with the `DEMO_TOKEN` env var) so you can see substitution and redaction working without any setup.

## Development

```sh
cargo fmt                 # format (run before finishing)
cargo clippy --release    # lint; must pass with zero warnings
cargo test --release      # run unit tests (parser + vars)
./target/release/http-client demo   # offline end-to-end self-test
```

Verification workflow for any change: `cargo fmt && cargo clippy --release && cargo test --release`, then run `demo` or a manual `serve` + `run` round trip. Tests must not require network access — use the embedded echo server or unit tests.

Architecture is documented in [AGENTS.md](AGENTS.md): `main.rs` (CLI + handlers), `parser.rs` (`.http` parsing), `vars.rs` (secret injection + redaction), `runner.rs` (execution + output), `server.rs` (echo server). No new dependencies: reqwest with `rustls-no-provider` + `ring`, `tiny_http` for the server, `clap` for the CLI.
