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

Using codex (using GPT-6-Astra high) I prompted the following:

> Read the coding challange at ~/Downloads/Rust Coding Challenge.pdf. I started
> working on an implementation. Check the current solution that I wrote in the
> main.rs. Check for correctness issues. Do not report on the known issues yet
> (missing docs, performance and cross platform incompatibiility).

Codex identified the following issues:

> 1. [P1] Valid chargebacks fail parsing — src/main.rs:95 The parser matches
>    "Chargeback", but the input specifies lowercase "chargeback". Any valid
>    chargeback aborts the entire run before processing transactions.
>
> 2. [P1] Disputes can affect another client’s funds — src/main.rs:164 The
>    dispute branch never verifies that the referenced transaction belongs to
>    the supplied client. With client 1 depositing 10 and client 2 depositing
>    20, dispute,2,1, incorrectly holds 10 from client 2. A subsequent
>    resolve,1,1, panics because it subtracts from client 1’s empty held
>    balance. Validate ownership before modifying balances or recording the
>    dispute.
>
> 3. [P1] Spending a deposit prevents its dispute — src/main.rs:171 Depositing
>    10, withdrawing 8, then disputing the deposit leaves available=2, held=0.
>    The specified dispute operation requires available=-8, held=10, total=2; it
>    has no sufficient-funds condition. This also prevents the later chargeback.
>    Supporting this requires a balance representation that permits negative
>    available funds.
>
> 4. [P2] Whole-number amounts are rejected — src/main.rs:57 deposit,1,1,1 fails
>    because parsing requires a decimal point. “Up to four places past the
>    decimal” includes zero fractional places. Treat a missing fractional part
>    as zero.
>
> 5. [P2] Resolved transactions cannot be disputed again — src/main.rs:160 The
>    existence check rejects every previously disputed transaction, including
>    those marked Resolved. A deposit of 10, followed by dispute → resolve →
>    dispute, ends with available=10, held=0. Resolution makes the transaction
>    no longer disputed; only chargeback is described as final. Check the stored
>    status when deciding whether to accept another dispute.
>
> 6. [P2] Amount and balance arithmetic can overflow — src/main.rs:70,
>    src/main.rs:148 1844674407370955.1616 panics during parsing. Separately,
>    depositing 1844674407370955.1615 and then 0.0001 panics when updating the
>    balance. These inputs meet the stated decimal precision constraint, which
>    sets no magnitude limit. Use checked arithmetic and return an explicit
>    error for unsupported values.
>
> 7. [P2] Output failures can be silently reported as success — src/main.rs:238
>    The CSV writer buffers output, but report() returns without explicitly
>    flushing it. Redirecting a small report to /dev/full produces exit status 0
>    with no error. Call writer.flush()? before returning so final write errors
>    propagate.

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

Once I finalized how processor should work, I moved most things into
`src/lib.rs` so test will be able to use them. Once that was done, I prompted
codex with the following:

> I ended up with the following design decisions: 1, Allow negative balances for
> dispute handling. 2, Allow multiple dispute attempts. 3, Only allow dispute
> resolution for deposits. 4, Disputes should still be handled even when an
> account has been frozen. The code has been updated. I created a lib.rs that
> contains exports that are needed for tests/. Could you write and test the
> correctness of the implementation? Do not write tests for input/output
> handling just yet.

And what I got back was:

> Both debug and release runs produced 26 passes and 3 failures. Tests covering
> all four design decisions passed.
>
> The failures expose arithmetic bugs:
>
> • Deposit conversion (src/lib.rs:177): depositing i64::MAX + 1 units produces
> a negative balance. • Withdrawal conversion (src/lib.rs:182): withdrawing
> u64::MAX units from an empty account credits it with one unit. • Balance
> accumulation (src/lib.rs:177): depositing i64::MAX, then one additional unit,
> panics in debug and wraps negative in release.
>
> These occur directly in transaction processing. I left the failing regression
> tests active and the processing logic unchanged.

It is fairly unrealistic to exceed the current i64::{MAX,MIN} boundaries, but
addressing this is easy with checked_{add,sub,mul} operations, so I followed up
with the following:

> • Added HwErrors::ArithmeticError. • Replaced unchecked amount arithmetic and
> casts with checked methods and try_from. • Propagated errors through both
> processing methods and the CLI. • Ensured arithmetic failures preserve
> balances and dispute status; batches stop at the first error.
>
> Updated regression tests: all 32 pass in debug and release builds.

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

I than asked codex about remaining issues and it wrote:

> • Two previously identified items remain:
>
> • Empty input still produces no output header — report
> (src/payment_processor.rs:179). Confirmed with header-only input.
> .has_headers(true) emits headers only when the first struct is serialized. You
> already deferred this.
>
> • Input is still loaded entirely before processing — get_transactions
> (src/lib.rs:53). This remains the known memory-efficiency concern.

The solution suggested was to write the headers explicitly before the csv
processing loop. I integrated it's solution.

I then wrote:

> Implemented your header suggestion. Now that the whole io part is fixed, write
> tests that test the whole flow, from parsing, processing and outputting.
> Should be in the tests/ folder, single file.

It added 16 e2e tests. I checked the tests aganist the requirements (and made
codex check it once again), but found no issues.
