TX_HSH=$1

./target/release/mev-inspect \
    -u http://localhost:8080 \
    --cache ./cache \
    --db-cfg postgresql://will@localhost/mev_inspections \
    -r \
    tx $TX_HSH 

# createdb mev_inspections
# psql -U will -W mev_inspections (no password)
# postgresql://will@localhost/mev_inspections
