# Repository Guide

This is a single Rust 2024 crate for a CSV payments engine. The CLI contract is
`cargo run -- transactions.csv > accounts.csv`.

## Working Style

- Make the smallest effective change and avoid touching unrelated code.
- Prefer simple, clear code over clever abstractions.
- Prefer iterator methods when they improve clarity; avoid complex chains.
- Prefer associated functions when there is a natural type association.
- Preserve public APIs unless the task explicitly calls for API changes.
- Avoid incidental renames, formatting churn, and broad rewrites.
- Define supporting types before the types that contain them, and keep related
  implementations near their types where practical.
- Add brief comments only where the intent would otherwise be hard to infer.

## Project Constraints

- Preserve the design decisions documented in `logs/iterations.md` unless the
  task calls for changing them.
- Keep amounts exact: integer units of 0.0001, checked balance arithmetic, and
  nonnegative transaction amounts validated at construction.
- Preserve transaction order and keep account updates atomic on arithmetic
  failure. Streaming input still requires retaining history for disputes.
- Keep CSV output on stdout and diagnostics on stderr. Use portable I/O.
- Generated benchmark files belong in `assets/` with the `test_` prefix and
  must remain Git-ignored. Generator usage is documented in `README.md`.

## Verification

- Use `cargo check --tests` for compilation checks and `cargo fmt --check` for
  formatting. Do not reformat unrelated code.
- Run tests relevant to behavior changes: `cargo test --test payment_processor`
  for processing, `cargo test --test cli` for the full CLI flow, or `cargo test`
  for both. Add tests for meaningful behavior changes when appropriate; avoid
  redundant tests and respect requests not to add them.
- For performance changes, benchmark the release binary directly on the same
  generated input and seed. Compare runtime and peak memory, and verify that
  account outputs remain equivalent regardless of row order.
