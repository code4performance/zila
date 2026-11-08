#!/bin/sh
# TLS and SCRAM only from the network; trust only inside the container (for health checks).
cat > "$PGDATA/pg_hba.conf" <<HBA
local   all all                trust
hostssl all all 0.0.0.0/0      scram-sha-256
hostssl all all ::/0           scram-sha-256
host    all all 0.0.0.0/0      reject
host    all all ::/0           reject
HBA
