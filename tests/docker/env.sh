# source tests/docker/env.sh   (after: docker compose -f tests/docker/compose.yml up -d --wait)
D="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
export BRASA_TEST_POSTGRES="postgres://app:Brasa_app_dev_1@127.0.0.1:55432/shop?sslmode=require&sslrootcert=$D/certs/ca.pem"
export BRASA_TEST_SQLSERVER="sqlserver://sa:Brasa_dev_Sql_1@127.0.0.1:51433/shop?encrypt=off"
export BRASA_TEST_SQLSERVER_STRICT="sqlserver://sa:Brasa_dev_Sql_1@127.0.0.1:51434/shop?encrypt=strict&cafile=$D/certs/ca.pem"
export BRASA_TEST_TLS="localhost:55443"
export BRASA_TEST_TLS_ALPN="localhost:55444"
export BRASA_TEST_CA="$D/certs/ca.pem"
export BRASA_TEST_SQLSERVER_AI="sqlserver://ai:ai@127.0.0.1:51433/njai?encrypt=off"
