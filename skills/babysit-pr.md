---
name: babysit-pr
description: Monitor and harden a LiteLLM Agent Platform pull request until CI and Bugbot are clean.
---

# Babysit A Pull Request

Use this skill when asked to babysit, watch, fix, or merge a PR for this repo.
Stay with the PR until it is merge-ready, or until an external blocker makes
progress impossible.

## Rules

- Always read `AGENTS.md` and `CODING_STANDARDS.md` before making code changes.
- Do not stage generated UI output such as `src/ui/out/` or `src/ui/.next/`.
- Do not merge unless the user asked for merge behavior.
- If merging is requested, merge only after all gates below are satisfied.
- Bugbot must be explicitly mentioned in a PR comment. A pushed commit alone is
  not enough.

## Start

1. Identify the PR:
   ```bash
   gh pr view --json number,url,headRefName,headRefOid,mergeStateStatus,statusCheckRollup
   ```
2. If no PR exists and the user asked for one, push the branch and create it:
   ```bash
   git push -u origin HEAD
   gh pr create --fill
   ```
3. Trigger Bugbot:
   ```bash
   gh pr comment <PR_NUMBER> --body '@bugbot please review this PR.'
   ```

## Required Local Verification

Run the checks relevant to the touched files before every push:

```bash
cargo fmt --all --check
python3 scripts/check_code_size.py
cargo test
cargo clippy --all-targets --locked -- -D warnings
ruff check .
git diff --check
```

If UI files under `src/ui/` changed, also run:

```bash
cd src/ui
npm run lint
npm run build
```

If a flow depends on Postgres, create a disposable database and run the focused
test with `TEST_DATABASE_URL`, then drop the database.

## Review Loop

Poll CI and review threads after every push:

```bash
gh pr view <PR_NUMBER> \
  --json mergeStateStatus,statusCheckRollup,headRefOid
```

Fetch unresolved review threads:

```bash
gh api graphql -f query='
query($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      reviewThreads(first: 100) {
        nodes {
          id
          isResolved
          comments(first: 1) {
            nodes { author { login } path body }
          }
        }
      }
    }
  }
}
' -f owner='LiteLLM-Labs' -f repo='litellm-agent-platform-2' -F number=<PR_NUMBER>
```

For each unresolved Bugbot thread:

1. Read the full comment and reproduce or reason through the failure.
2. Make the smallest repo-consistent fix.
3. Run the required local verification.
4. Commit and push.
5. Resolve the thread only after the fix is pushed:
   ```bash
   gh api graphql -f query='
   mutation($thread: ID!) {
     resolveReviewThread(input: { threadId: $thread }) {
       thread { id isResolved }
     }
   }
   ' -f thread='<THREAD_ID>'
   ```
6. Ask Bugbot to re-review:
   ```bash
   gh pr comment <PR_NUMBER> --body '@bugbot please re-review the latest fixes.'
   ```

Repeat until Bugbot has completed and there are zero unresolved Bugbot threads.

## Merge Gate

The PR is mergeable only when all of these are true:

- `mergeStateStatus` is clean enough for GitHub to merge.
- Required CI checks are terminal and successful.
- Cursor Bugbot has completed after the latest head commit.
- There are zero unresolved Bugbot review threads.
- The working tree contains no accidental or generated files staged for commit.

If the user requested merge behavior, merge only after the gate passes:

```bash
gh pr merge <PR_NUMBER> --squash --delete-branch
```

If any gate fails, keep fixing and re-requesting `@bugbot` review instead of
merging.
