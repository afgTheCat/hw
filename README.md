# Payments processor

Take-home payment processor assignment.

## Running it

Tested on `1.98.1-stable` and `1.100.0-nightly`.

```sh
cargo run -- assets/test.csv
```

Both relative and absolute paths were tested.

## Design decisions

I wrote a [design journal](docs/design_journal.md) as I was working on the
problem. It includes the prompts used for AI-assisted development, responses,
and the reasoning behind implementation decisions and experiments.

Design priorities, in decreasing order:

- correctness
- maintainability/clarity
- performance

The following design choices are worth highlighting:

| What                                             | Why                                                                        |
| ------------------------------------------------ | -------------------------------------------------------------------------- |
| Amounts are stored as integers                   | Avoids floating-point rounding errors.                                     |
| Input is streamed                                | Avoids collecting the entire input. Transaction history is still retained. |
| Dispute status is stored inside each transaction | Simplifies bookkeeping without increasing transaction size (on x86_64).    |
| Use FxHashMap instead of the default HashMap     | Improved performance on the measured workload.                             |

## Assumptions

We do not assume that the input is well formatted, but we assume that it is not
malicious (i.e. we do not really take adversarial hashing into consideration).
On top of that, some requirements were not clear to me, so I am making the
following assumptions:

| Assumption made                                                 | Why                                                                                                                    |
| --------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Allow negative balances for dispute handling.                   | Stripe explains that chargebacks can cause negative balances.                                                          |
| Resolved deposits may be disputed again. Chargebacks are final. | Resolution clears the active dispute, while chargeback is the terminal state.                                          |
| Only deposits can be disputed.                                  | This is my interpretation of the challenge's fund movements, rather than an explicit restriction in the specification. |
| Disputes are still handled even if the account is frozen.       | Stripe states that disputes can arise even after an account is closed.                                                 |

Malformed input and arithmetic overflow stop processing with an error.
Inapplicable transaction operations are ignored.

## Testing

Run the test suite with:

```sh
cargo test
```

We currently have two larger test files:

- `tests/cli.rs`: launches the CLI and tests the complete flow from CSV parsing
  through processing to output.
- `tests/payment_processor.rs`: tests the processor directly, covering balance
  updates, dispute state transitions, ownership checks, frozen accounts, and
  arithmetic overflow.

The tests were generated entirely with AI and reviewed.

## Performance testing

You can generate a large test file with a roughly realistic transaction mix:

```sh
cargo run --release --bin generate_test_file
```

The generated file, `assets/test_1m_1.csv`, contains 1,000,000 transactions. The
design journal describes performance optimizations evaluated by running
`hyperfine` on this file:

```sh
cargo build --release --bin payments-engine
hyperfine --shell=sh --warmup 3 --runs 10 \
    'target/release/payments-engine assets/test_1m_1.csv > /dev/null'
```

The generator was also AI-generated, its prompt is included in the journal.

## Possible improvements

The samply recording showed roughly 70% of sampled CPU time in CSV reading and
deserialization, and 20% in HashMap operations. Potential optimizations to
benchmark include:

- multithreading
- custom parsing
- memory-mapped input and SIMD

Articles about tackling the One Billion Row Challenge (1BRC) in Rust offer
further ideas to explore.
