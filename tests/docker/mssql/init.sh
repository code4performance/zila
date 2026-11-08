#!/bin/sh
# One-shot: waits for the server, then runs init.sql with sqlcmd (shipped in the image).
set -e
S=/opt/mssql-tools18/bin/sqlcmd
i=0
until $S -S "$HOST" -U sa -P "$MSSQL_SA_PASSWORD" -C -Q "select 1" >/dev/null 2>&1; do
  i=$((i+1)); [ $i -gt 60 ] && { echo "server not ready"; exit 1; }
  sleep 2
done
$S -S "$HOST" -U sa -P "$MSSQL_SA_PASSWORD" -C -b -i /init.sql
echo "seeded $HOST"
