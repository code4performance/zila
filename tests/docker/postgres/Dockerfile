# PostgreSQL 19 (newest tag available: see design.md decision 6) with pgvector built from source.
ARG PG_TAG=19rc1
FROM postgres:${PG_TAG}
ARG PGVECTOR_VERSION=v0.8.7
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential git ca-certificates postgresql-server-dev-$(echo "$PG_MAJOR") \
 && git clone --depth 1 --branch ${PGVECTOR_VERSION} https://github.com/pgvector/pgvector.git /tmp/pgvector \
 && make -C /tmp/pgvector OPTFLAGS="" && make -C /tmp/pgvector install \
 && apt-get purge -y build-essential git postgresql-server-dev-$(echo "$PG_MAJOR") && apt-get autoremove -y \
 && rm -rf /tmp/pgvector /var/lib/apt/lists/*
