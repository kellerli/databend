#!/usr/bin/env bash

CURDIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$CURDIR"/../../../shell_env.sh

echo "create database if not exists test_attach_only;" | $BENDSQL_CLIENT_CONNECT

# mutation related enterprise features

echo "create table test_attach_only.test_json(id int, val json) 's3://testbucket/admin/data/' connection=(access_key_id ='minioadmin' secret_access_key ='minioadmin' endpoint_url='${STORAGE_S3_ENDPOINT_URL}');" | $BENDSQL_CLIENT_CONNECT
echo "insert into test_attach_only.test_json values(1, '{\"a\":33,\"b\":44}'),(2, '{\"a\":55,\"b\":66}')" | $BENDSQL_CLIENT_CONNECT
storage_prefix=$(mysql -uroot -h127.0.0.1 -P3307  -e "set global hide_options_in_show_create_table=0;show create table test_attach_only.test_json" | grep -i snapshot_location | awk -F'SNAPSHOT_LOCATION='"'"'|_ss' '{print $2}')
echo "attach table test_attach_only.test_json_read_only 's3://testbucket/admin/data/$storage_prefix' connection=(access_key_id ='minioadmin' secret_access_key ='minioadmin' endpoint_url='${STORAGE_S3_ENDPOINT_URL}') READ_ONLY;" | $BENDSQL_CLIENT_CONNECT

echo "create virtual column should fail"
echo "CREATE VIRTUAL COLUMN (val['a'], val['b']) FOR test_attach_only.test_json_read_only" | $BENDSQL_CLIENT_CONNECT

echo "alter virtual column should fail"
echo "ALTER VIRTUAL COLUMN (val['k1']) FOR test_attach_only.test_json_read_only" | $BENDSQL_CLIENT_CONNECT

echo "drop virtual column should fail"
echo "DROP VIRTUAL COLUMN FOR test_attach_only.test_json_read_only" | $BENDSQL_CLIENT_CONNECT

echo "refresh virtual column should fail"
echo "REFRESH VIRTUAL COLUMN FOR test_attach_only.test_json_read_only" | $BENDSQL_CLIENT_CONNECT

# vacuum
echo "vacuum table"

echo "vacuum table should fail"
echo "VACUUM TABLE test_attach_only.test_json_read_only RETAIN 0 HOURS;" | $BENDSQL_CLIENT_CONNECT

echo "vacuum drop table from db should not include the read_only attach table"
# drop & vacuum
echo "drop table test_attach_only.test_json_read_only" | $BENDSQL_CLIENT_CONNECT
echo "vacuum drop table from test_attach_only retain 0 hours" | $BENDSQL_CLIENT_CONNECT
# attach it back
echo "attach table test_attach_only.test_json_read_only 's3://testbucket/admin/data/$storage_prefix' connection=(access_key_id ='minioadmin' secret_access_key ='minioadmin' endpoint_url='${STORAGE_S3_ENDPOINT_URL}') READ_ONLY" | $BENDSQL_CLIENT_CONNECT
echo "expect table data still there"
echo "select * from test_attach_only.test_json_read_only order by id" | $BENDSQL_CLIENT_CONNECT


