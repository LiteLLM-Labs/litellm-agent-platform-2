# AGENTS.md

Before making implementation changes, read and follow the repo-wide
[`CODING_STANDARDS.md`](./CODING_STANDARDS.md).

## First-time setup

Run once after cloning to activate the committed git hooks:

```bash
git config core.hooksPath .githooks
```

The pre-commit hook keeps `model_prices_backup.json` in sync with the
upstream litellm JSON on every commit. It warns and skips silently if
the network is unavailable — it never blocks a commit.

## Cursor Cloud specific instructions

### Product overview

Single deployable **litellm-rust** gateway: Rust/Axum API + embedded Next.js
UI (`src/ui/out`). Default dev URL: `http://127.0.0.1:4000`. See
[`docs/contributing.md`](./docs/contributing.md) for the contributor quickstart.

### Toolchain

- **Rust 1.96+** is required (some crates need `edition2024`). If `cargo build`
  fails with that error, run `rustup default stable`.
- **Node.js + npm** for the UI (`src/ui/package-lock.json`).
- **PostgreSQL 16** for the managed-agents UI and DB-backed features. Gateway
  health/models work without Postgres; UI routes that need the DB return 503.

### PostgreSQL (one-time per VM)

```bash
sudo pg_ctlcluster 16 main start
sudo -u postgres createuser -s litellm 2>/dev/null || true
sudo -u postgres createdb -O litellm litellm_rust_dev 2>/dev/null || true
sudo -u postgres psql -c "ALTER USER litellm WITH PASSWORD 'devpass';"
```

Use `DATABASE_URL=postgres://litellm:devpass@localhost/litellm_rust_dev`.

### Config

```bash
cp config.yaml.example config.yaml
```

`config.yaml` is gitignored. For local dev, keep **only one wildcard** model
route (`anthropic/*` *or* `openai/*`); two wildcards fail at startup with
`only one wildcard model route is supported`. Strip `mcp_servers` and `agents`
from the example unless you set all referenced env vars (`E2B_API_KEY`, MCP
keys, etc.).

Minimal dev env vars:

```bash
export LITELLM_MASTER_KEY=sk-dev-master-key
export ANTHROPIC_API_KEY=sk-ant-...        # placeholder OK for UI-only dev
export DATABASE_URL=postgres://litellm:devpass@localhost/litellm_rust_dev
```

### Run (integrated UI + API)

```bash
cd src/ui && npm run build && cd ../..
cargo run -- serve --config config.yaml --host 127.0.0.1 --port 4000
```

Log in at `/login` with `LITELLM_MASTER_KEY`. `npm run dev` in `src/ui` serves
Next.js on port **3210** without proxying to the gateway — prefer the integrated
build path above for full-stack dev.

### Lint and test

See [`.github/workflows/rust-checks.yml`](./.github/workflows/rust-checks.yml):

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python3 scripts/check_code_size.py
cd src/ui && npm run lint
```

Postgres integration tests use `TEST_DATABASE_URL` when set (same DB as above
is fine). Most tests use wiremock and need no real API keys.
