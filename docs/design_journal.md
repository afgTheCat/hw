The project-related conversations, including the full questions and follow-ups,
are collected in the [AI transcript](ai_transcript.md). This journal retains the
development notes, design decisions, and benchmark results.

# First commit - Make it work

This commit is entirely hand written, no AI tools are used. I only used docs.rs
in order to read about the csv crate. The reason for that is just so I can have
fun with the challange, not anything ideological/get to know the problem better.
Priority number one is to have something working (passing the test file).

## Minor notes

- Transcation amounts should be stored in an u64 instead of f32/f64. This is in
  order to avoid numerical errors relating to floating point arithmetics. This
  is an interview question that I used to ask for new hires previously.
- The initial implementation will be far from optimal. We are allocating
  (potentially fairly large vecs) unnecessarily, we are not really
  pipelining/threading etc. Will see how much time I have to implement this.
- At this stage, I only use the lsp for fromating/static analysis. I will run
  clippy later on and create a rustfmt config file
- On top of csv and serde, I introduced anyhow and thiserror just to make error
  handling more convinient
- I am not really writing comments/documentation. That comes later. I feel like
  I am going to introduce clap just for this reason.
- Expect this code to be ugly as the right data structures are being refined
- Expect no platform compatibiliy, that comes later

## Questsions

- Not explicitly stated, but I assume disputes can only happen whith deposits
- What happens if we dispute a tranasciton, but the client funds cannot further
  be decreased? For now, we will assume that that no dispute can be raised, but
  that is not really a good strategy imo
- I assume that a locked account should not be able to deposit/withdraw money.
  Should it? How about disputes?
- Can a tranasction that has been disputed and resolved once be disputed again?
  For simplicity, I will make all transctions be disputable only once?

# Second commit - Making it correct, the logic

I used Codex (GPT-6-Astra high) to review correctness. The prompt and the
numbered findings are recorded in the
[AI transcript](ai_transcript.md#correctness-review).

#1, #2 and #7 are simple oversights that I corrected immediately. #3 and #5
probably stems from not understanding how payment providers usually handle
disputes. #4 and #6 are simple parsing concerns.

At this point I did some research how real payment processors usually handle
disputes and decided to go with the following design based on Stripe.

- Allow negative balances for dispute handling.
  [Stripe: Accounting for negative balances](https://docs.stripe.com/connect/account-balances#accounting-for-negative-balances)
  explains that chargebacks can cause negative balances.
- Allow multiple dispute attempts.
  [Stripe: Receive multiple disputes](https://docs.stripe.com/disputes/how-disputes-work#receive-multiple-disputes)
  documents multiple disputes against the same payment, for example when the
  issuer receives new information or a different reason code is used.
- Only allow dispute resolution for deposits. This is not realistic, but the
  challenge only addresses this case.
  [Stripe: Issuing disputes](https://docs.stripe.com/issuing/purchases/disputes)
  shows that cardholders can dispute outgoing purchases too.
- Disputes should still be handled even when an account has been frozen.
  [Stripe: Disputes after account closure](https://support.stripe.com/questions/refunds-and-disputes-after-closing-a-stripe-account)
  states that disputes can arise even after an account is closed.

Once I finalized how the processor should work, I moved most things into
`src/lib.rs` so tests could use them. I asked Codex to write processor tests;
the initial run exposed three arithmetic failures. The request and results are
in the [AI transcript](ai_transcript.md#processor-tests-and-arithmetic-safety).

It is fairly unrealistic to exceed the current i64::{MAX,MIN} boundaries, but
addressing this is easy with checked_{add,sub,mul} operations, so I asked Codex
to address them. The
[implementation summary](ai_transcript.md#processor-tests-and-arithmetic-safety)
records checked arithmetic, error propagation, and atomic balance updates.

I did a minimal refactor at this point, creating a module for the transaction.
Reason is two fold:

- clarity
- now that trx amounts are `i64`, we risk making the illegal stated
  representable. The easiest way to fight this is to just use private fields and
  restrict how Transactions can be constructed.

# Third commit - Making it correct, input/output

The following things I observed:

- We do not have a valid parser, whole numbers (without dots) are ignored
- Since the total value is only calculated during output, we can overflow there
  as well (available = i64::Max, held = i64::MAX). This is easily preventable
  (and probably should be prevented). Instead of book-keeping, I think we could
  use a larger number at the end (like i128) to write.
- I lazily just wrote to csv to `/dev/stdout`, which is not platform
  independent. Now we are writing to std::io::stdout().

A follow-up review identified missing output headers for empty input and
unnecessary input collection. The recorded findings are in the
[AI transcript](ai_transcript.md#csv-reporting-and-cli-tests).

The solution suggested was to write the headers explicitly before the csv
processing loop. I integrated it's solution.

I then asked Codex for end-to-end tests covering parsing, processing, and
output. The request is in the
[AI transcript](ai_transcript.md#csv-reporting-and-cli-tests).

It added 16 e2e tests. I checked the tests aganist the requirements (and made
codex check it once again), but found no issues.

# Forth commit - Making it fast #1

At this point we should have a working and correct implementation. Time to make
it fast.

## Why this is slow

There are a couple of reasons I suspect why the current aproach is not optimal.
These are:

- Instead of buffering the csv content we could stream it and process it in
  place
- HashMap in rust is cryptographically secure
- Another approach would be to mmap the csv instead of streaming (lot of the
  1brc used something like that)
- we could paralell process it by exploiting the fact that different clients can
  be processed independent of each other (could be interesting)

## Getting the baseline

First thing we are going to do is create a test file. To do that, we are going
to create `src/bin/generate_test_file.rs`. I let Codex handle this one; the
[generator request](ai_transcript.md#benchmark-input-generator) records the
transaction mix, reference rules, client replacement, and output requirements.

At this point I also decided to introduce an AGENTS.md, as I suspected now that
I have a good understanding of the problem, I will mostly use codex from now on.

```sh
cargo run --release --bin generate_test_file
   Compiling payments-engine v0.1.0 (/home/gabor/projects/hw)
    Finished `release` profile [optimized] target(s) in 0.42s
     Running `target/release/generate_test_file`
Generated 1000000 rows with seed 1: 100 active, 50 frozen, 150 total clients
Deposits: 550685, withdrawals: 439354, disputes: 6980, resolves: 2926, chargebacks: 55
Wrote /home/gabor/projects/hw/assets/test_1m_1.csv
```

Using hyperfine with some warmup on 10 runs (and piping the output to
`/dev/null` so it is less annoying).

```
cargo build --release
hyperfine --shell=sh --warmup 3 --runs 10 \
    'target/release/payments-engine assets/test_1m_1.csv > /dev/null'

Benchmark 1: target/release/payments-engine assets/test_1m_1.csv > /dev/null (10 runs)
                   mean ±     σ    min …   max
  Wall Time [ms]  330.9 ±   5.3  324.0 … 339.4
  Memory [MiB]    215.0 ±   0.1  214.8 … 215.3
```

What a
[coincidence](https://matklad.github.io/2026/10/05/benchmark-milliseconds.html)
!

## Improvement: Streaming

An idea I had in mind, that is a potentially a quick win is streaming. The main
processing loop becomes this (imo it was too beutiful not to dump it here):

```rust
pub fn process_all_transactions<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    let mut payment_processor = PaymentProcessor::default();
    let mut reader = ReaderBuilder::new().trim(Trim::All).from_path(path)?;
    for record in reader.deserialize::<CsvInputRow>() {
        let record = record?;
        let transaction = Transaction::try_from(record)?;
        payment_processor.process_transaction(transaction)?;
    }
    payment_processor.report()?;
    Ok(())
}
```

As a result of not allocating intermediate `Vec`s, memeory consumption is
reduced.

```sh
git:(main) ✗ hyperfine --shell=sh --warmup 3 --runs 10 \
'target/release/payments-engine assets/test_1m_1.csv > /dev/null'

Benchmark 1: target/release/payments-engine assets/test_1m_1.csv > /dev/null (10
runs) mean ± σ min … max Wall Time [ms] 299.2 ± 4.1 293.0 … 305.5 Memory [MiB]
101.6 ± 0.2 101.2 … 101.8
```

# Fifth commit - Making it fast #2

The next improvement I wanted to test out, is using faster data structures.
Rust's hashmap is famously cryptographically secure, and we don't need it to be.
I asked Codex for a recommendation, then asked it to integrate and benchmark the
three alternatives. Both requests are in the
[AI transcript](ai_transcript.md#hashmap-comparison-and-integration).

Then I went to store to grab some snacks. The results were:

| Map             | Time, mean ± σ | Mean peak memory | Time reduction |
| --------------- | -------------: | ---------------: | -------------: |
| Default HashMap | 298.3 ± 2.6 ms |        101.6 MiB |              — |
| FxHashMap       | 271.0 ± 3.2 ms |        101.6 MiB |           9.2% |
| AHashMap        | 273.8 ± 2.1 ms |        101.7 MiB |           8.2% |
| FnvHashMap      | 272.0 ± 3.4 ms |        101.8 MiB |           8.8% |

I then asked codex to integrate FxHashMap.

## Flamegraph

Next I checked the flamegraph using samply:

```sh
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release --bin payments-engine
samply record target/release/payments-engine assets/test_1m_1.csv > /dev/null
```

At this point, roughly 70 percent of the time is spent deserializing the csv and
20 percent is spent on hashmap operations. Before trying to further optimize
this, I want to have a round of code cleanup so the code is presentable. I also
added the perf related things to the gitignore.

# Sixth commit - Code improvements

Next I wanted to use `cargo clippy` with padentic so added this to `Cargo.toml`.

```toml
[lints.clippy]
pedantic = { level = "warn", priority = -1 }
```

And then:

```sh
cargo clippy --fix --allow-dirty
```

I then asked Codex to clean up remaining Clippy issues
([request](ai_transcript.md#clippy-cleanup)).

Next I simplified the `PaymentProcessor` the following way: instead of having
the disputes be stored separately, each transaction will have a dispute status.
Performance decresed slightly due to storing the status next to the Transaction.
Then I realized that the transaction could fit the status with its padding, so I
migrated the status to the transaction, increasing perf slightly. The related
implementation requests are in the
[AI transcript](ai_transcript.md#dispute-state-and-account-operations).

```sh
hyperfine --shell=sh --warmup 3 --runs 10 \
    'target/release/payments-engine assets/test_1m_1.csv > /dev/null'

Benchmark 1: target/release/payments-engine assets/test_1m_1.csv > /dev/null (10 runs)
                   mean ±     σ    min …   max
  Wall Time [ms]  261.4 ±   2.1  259.5 … 266.1
  Memory [MiB]    101.1 ±   0.2  100.8 … 101.3

```

After this I asked Codex to suggest and implement clearer names for the account,
transaction, CSV row types, and processing methods. The suggestions and
implementation request are in the
[AI transcript](ai_transcript.md#naming-cleanup).

# Seventh commit - writing the README.md

Writing the README.md, some minor tweaks.
