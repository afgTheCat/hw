# First commit

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
