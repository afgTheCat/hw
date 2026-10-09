# Payments engine

Generate a reproducible benchmark input with one million transaction rows:

```sh
cargo run --release --bin generate_test_file
```

The optional argument is a random seed (default: `1`). The output is
`assets/test_1m_<seed>.csv`; generated `assets/test_*` files are ignored by Git.
All transactions are generated and processed in memory before the CSV is
written.

Generation starts with clients 1 through 100. The random mix is 55% deposits,
44% withdrawals, 0.7% disputes, 0.2947% resolves, and 0.0053% chargebacks.
Disputes normally reference eligible deposits, and resolves and chargebacks
normally reference pending disputes (95% when available). Other references use a
nonexistent transaction ID. Resolved deposits can be disputed again.

Every newly frozen client is replaced with a new client and attempts a deposit
and withdrawal after freezing. Later deposit and withdrawal rows also select
frozen clients 5% of the time. Replacement deposits and these mandatory attempts
count toward the one million rows. The final population is approximately 150
clients: 100 active and about 50 frozen, varying by seed. Generation logs the
actual client and transaction counts to stderr.
