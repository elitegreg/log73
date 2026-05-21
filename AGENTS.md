## Planning and approval workflow

Before changing code, do not edit files.

First:

1. Inspect the relevant files.
2. Produce a development plan.
3. List the exact files expected to change.
4. Explain risks, alternatives, and tests to run.
5. Wait for explicit approval.

Only apply changes after I say one of:

- "approved"
- "apply the plan"
- "go ahead"

If the requested change is ambiguous, ask clarifying questions before planning.

When a github issue is fixed or implemented, note that the issue is closed in both the git commit message and PR text (Closes #1)

DB Migrations: Always assume I'm starting from a fresh database and don't need migrations, but if you think a migration is needed in a particular case, ask me, but make the initial plan with no migration.
