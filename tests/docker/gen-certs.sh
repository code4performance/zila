#!/bin/sh
# Creates a development CA and one server certificate (localhost, 127.0.0.1) in /certs.
# Run by the `certs` service of compose.yml (image alpine/openssl); skipped when already done.
set -e
cd /certs
if [ -f ca.pem ] && [ -f server.crt ] && [ -f server.key ]; then echo "certs exist"; exit 0; fi
openssl genrsa -out ca.key 2048
openssl req -x509 -new -key ca.key -sha256 -days 825 -subj "/CN=Zila Lang Dev CA" -out ca.pem
openssl genrsa -out server.key 2048
openssl req -new -key server.key -subj "/CN=localhost" -out server.csr
cat > ext.cnf <<EXT
subjectAltName=DNS:localhost,IP:127.0.0.1
extendedKeyUsage=serverAuth
keyUsage=digitalSignature,keyEncipherment
EXT
openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial -days 825 -sha256 -extfile ext.cnf -out server.crt
rm -f server.csr ext.cnf ca.srl
chmod 644 ca.pem server.crt server.key ca.key
echo "certs created"
