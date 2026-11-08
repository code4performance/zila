# . tests\docker\env.ps1   (after: docker compose -f tests/docker/compose.yml up -d --wait)
$d = $PSScriptRoot -replace '\', '/'
$env:ZILA_TEST_POSTGRES = "postgres://app:Zila_app_dev_1@127.0.0.1:55432/shop?sslmode=require&sslrootcert=$d/certs/ca.pem"
$env:ZILA_TEST_SQLSERVER = "sqlserver://sa:Zila_dev_Sql_1@127.0.0.1:51433/shop?encrypt=off"
$env:ZILA_TEST_SQLSERVER_STRICT = "sqlserver://sa:Zila_dev_Sql_1@127.0.0.1:51434/shop?encrypt=strict&cafile=$d/certs/ca.pem"
$env:ZILA_TEST_TLS = "localhost:55443"
$env:ZILA_TEST_TLS_ALPN = "localhost:55444"
$env:ZILA_TEST_CA = "$d/certs/ca.pem"
$env:ZILA_TEST_SQLSERVER_AI = "sqlserver://ai:ai@127.0.0.1:51433/njai?encrypt=off"
