# AGENTS.md

Guidance for AI coding agents working in this repository.

## Project Overview

`http-client` is a Rust CLI debugging tool that executes `.http` files (REST Client / JetBrains HTTP format) and prints response information. It also embeds a local echo server used to test requests offline.

## Commands

Rust must be on PATH (`export PATH="$HOME/.cargo/bin:$PATH"` if installed fresh via rustup).

```sh
cargo build --release     # build binary to ./target/release/http-client
cargo test --release      # run unit tests (parser + vars tests)
cargo clippy --release    # lint; must pass with zero warnings
cargo fmt                 # format code (run before finishing)
cargo fmt --check         # verify formatting
./target/release/http-client demo   # end-to-end self-test (offline, no network)
```

Verification workflow for any change: `cargo fmt && cargo clippy --release && cargo test --release`, then run `demo` or a manual `serve` + `run` round trip.

## Architecture

Single binary crate. Modules in `src/`:

- `main.rs` — CLI definition (clap) and subcommand handlers: `run <file>` (execute `.http` file), `serve` (start echo server), `demo` (spawn ephemeral echo server and fire sample requests at it). Also installs the rustls ring crypto provider at startup.
- `parser.rs` — parses `.http` file content into `ParsedRequest { method, url, headers, body }`. Unit tests live here.
- `runner.rs` — executes a `ParsedRequest` via `reqwest::blocking::Client` and prints output (`→ request`, `← status · reason · duration · content-type`, pretty-printed JSON body). Owns ANSI color handling (`Colors::auto()` respects `NO_COLOR` and tty detection). All printed output passes through redaction with the list of substituted secret values.
- `vars.rs` — secret injection: builds a variable map from process env vars plus optional `--env-file` KEY=value files (file entries override env vars; map assembly is testable via `Secrets::from_maps`), substitutes `{{VAR}}` placeholders post-parse, records every substituted value for output redaction. Unit tests live here.
- `server.rs` — echo server built on `tiny_http`. Accepts any method/path, responds with JSON mirroring `{method, path, headers, bodyLength, body}` of the received request. `spawn()` binds an OS-assigned free port and reports it back over an mpsc channel.

## .http File Format

Handled by `src/parser.rs`:

- Requests separated by lines starting with `###` (rest of line is a label)
- Lines starting with `#` are comments (outside bodies)
- First meaningful line: `METHOD URL [HTTP/x.x]` — HTTP version token is accepted and ignored
- Header lines (`Name: value`) until the first blank line
- Everything after the blank line is the body (trimmed); absent if empty
- `{{VAR}}` placeholders in URLs, header names/values, and bodies are substituted after parsing (`vars.rs`), from process env vars and `--env-file` files. Unresolved or malformed placeholders (`{{`, `{{}}`) abort the run before any request is sent. Single-level only — no nested/escaped placeholders.

## Secrets & Redaction

- Secrets come from process env vars; `--env-file <FILE>` (repeatable) loads `KEY=value` files and takes precedence over env vars. No auto-detection of `.env` files.
- Substituted values are masked as `[redacted]` in printed requests, dry-run output, `--verbose` headers, and rendered response bodies; `--no-redact` disables.
- `run --request N` only substitutes the selected request(s), so unresolved variables in other requests don't block a single-request run.

## Conventions

- No code comments unless requested.
- Keep dependencies minimal; reqwest uses `rustls-no-provider` + `ring` because this machine has no cmake (aws-lc-rs default provider requires it).
- The client sends a 30s timeout and `http-client/0.1` user agent (`build_client` in `main.rs`).
- Tests must not require network access; use the embedded echo server or unit tests only.

## Gotchas

- `example.http` targets `127.0.0.1:8080` — start `http-client serve` before running it.
- `cargo remove`/re-adding dependencies can drop unrelated entries from `Cargo.toml`; verify deps after editing it manually.
- reqwest 0.13 renamed TLS features: use `rustls-no-provider` (+ explicit `rustls` crate with `ring` feature), not the old `rustls-tls`.
