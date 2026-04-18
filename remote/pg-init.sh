#!/usr/bin/env bash
# Start a local Postgres 16 using tmpfs. Data wiped on every boot.
set -euo pipefail

PGDATA=/run/pgdata
mkdir -p "$PGDATA"
chown -R postgres:postgres "$PGDATA"
chmod 700 "$PGDATA"

if [ ! -f "$PGDATA/PG_VERSION" ]; then
  sudo -u postgres /usr/lib/postgresql/16/bin/initdb -D "$PGDATA" -E UTF8 --locale=C >/dev/null
  cat >> "$PGDATA/postgresql.conf" <<EOF
listen_addresses = '127.0.0.1'
unix_socket_directories = '/tmp'
shared_buffers = 128MB
EOF
  echo "host all all 127.0.0.1/32 md5" >> "$PGDATA/pg_hba.conf"
fi

sudo -u postgres /usr/lib/postgresql/16/bin/pg_ctl -D "$PGDATA" -l /tmp/pg.log -o "-p 5432" start

# Wait
for i in $(seq 1 30); do
  if pg_isready -h 127.0.0.1 -p 5432 >/dev/null 2>&1; then break; fi
  sleep 0.5
done

sudo -u postgres psql -h 127.0.0.1 -c "DO \$\$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'dev') THEN CREATE ROLE dev LOGIN PASSWORD 'dev' SUPERUSER; END IF; END \$\$;" >/dev/null
sudo -u postgres psql -h 127.0.0.1 -tc "SELECT 1 FROM pg_database WHERE datname='app_dev'" | grep -q 1 \
  || sudo -u postgres createdb -h 127.0.0.1 -O dev app_dev

echo "postgres ready on 127.0.0.1:5432 (db=app_dev user=dev pass=dev)"
