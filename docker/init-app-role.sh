#!/bin/bash
# Creates the restricted role the app runtime uses. Only runs on first initialization
# against an empty data directory (the official Postgres image's
# docker-entrypoint-initdb.d convention) — existing deployments are unaffected.
#
# Permissions are not granted here — migration 0031 grants them, so every upgrade can
# add newly created tables. This script is only responsible for the role itself existing,
# with a password it can log in with.
set -euo pipefail

APP_PASSWORD="${UTOPIA_APP_DB_PASSWORD:-}"
if [ -z "$APP_PASSWORD" ]; then
    echo "UTOPIA_APP_DB_PASSWORD is not set, skipping restricted role creation; the app will run as the owner." >&2
    exit 0
fi

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<SQL
DO \$\$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'utopia_app') THEN
        CREATE ROLE utopia_app LOGIN PASSWORD '${APP_PASSWORD}';
        RAISE NOTICE 'Created restricted role utopia_app';
    END IF;
END
\$\$;
GRANT CONNECT ON DATABASE "$POSTGRES_DB" TO utopia_app;
SQL
