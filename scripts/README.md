# Scripts

This directory contains utility scripts for the LiteLLM Agent Platform.

## readiness_check.py

Comprehensive readiness check for the LiteLLM Agent Platform runtime PoC.

### Usage

```bash
# Basic readiness check (configuration, environment, build artifacts)
./scripts/readiness_check.py

# Include server runtime checks (requires server to be running)
./scripts/readiness_check.py --check-server

# Use custom config file
./scripts/readiness_check.py --config my-config.yaml

# Check against different server instance
./scripts/readiness_check.py --check-server --base-url http://localhost:8080
```

### What it checks

#### Critical checks (must pass)
- **Config File**: Validates config file exists and is readable
- **Environment Variables**: Required env vars (`LITELLM_MASTER_KEY`, `DATABASE_URL`)
- **Database**: Connection test if DATABASE_URL is available
- **Binary Build**: Verifies `lite` binary builds successfully
- **Server Health**: `/health` endpoint responds correctly (if `--check-server`)
- **API Endpoints**: Core API routes work with authentication (if `--check-server`)

#### Warning checks (recommended)
- **Optional Environment Variables**: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `E2B_API_KEY`
- **UI Build**: Static UI files are built and available
- **Capabilities**: Detailed endpoint and feature availability

### Exit codes
- `0`: All critical checks passed
- `1`: One or more critical checks failed

### Requirements
- Python 3.8+
- `psql` command (optional, for database connection testing)
- `cargo` (for build verification)

Use this script before deployment or when troubleshooting runtime issues to quickly identify configuration problems.

## check_code_size.py

Enforces code size limits for CI/CD. See file for details.