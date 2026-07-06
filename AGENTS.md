## Database migrations

DB Migrations: Always assume I'm starting from a fresh database and don't need migrations, but if you think a migration is needed in a particular case, ask me, but make the initial plan with no migration.

## Database query safety

Use safe query patterns for database access. Prefer static SQL with prepared statements and bound parameters, or a query builder. Do not construct SQL by formatting or interpolating user-provided values into SQL strings. If SQL identifiers must vary, select them from a hardcoded whitelist.
