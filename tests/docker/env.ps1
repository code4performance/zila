# . tests\docker\env.ps1   (after: docker compose -f tests/docker/compose.yml up -d --wait)
$d = $PSScriptRoot -replace '\', '/'
$env:BRASA_TEST_POSTGRES = "postgres://app:Brasa_app_dev_1@127.0.0.1:55432/shop?sslmode=require&sslrootcert=$d/certs/ca.pem"
$env:BRASA_TEST_SQLSERVER = "sqlserver://sa:Brasa_dev_Sql_1@127.0.0.1:51433/shop?encrypt=off"
$env:BRASA_TEST_SQLSERVER_STRICT = "sqlserver://sa:Brasa_dev_Sql_1@127.0.0.1:51434/shop?encrypt=strict&cafile=$d/certs/ca.pem"
$env:BRASA_TEST_TLS = "localhost:55443"
$env:BRASA_TEST_TLS_ALPN = "localhost:55444"
$env:BRASA_TEST_CA = "$d/certs/ca.pem"
$env:BRASA_TEST_SQLSERVER_AI = "sqlserver://ai:ai@127.0.0.1:51433/njai?encrypt=off"
