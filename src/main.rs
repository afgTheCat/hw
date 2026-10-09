use std::env::current_dir;

use payments_engine::{PaymentError::IncorrectAmountOfArgs, process_all_transactions};

fn main() -> anyhow::Result<()> {
    let input: Vec<_> = std::env::args().collect();
    let file = match input.as_slice() {
        [_, input] => input.clone(),
        [_, args @ ..] | args => {
            let args = args.to_vec();
            return Err(IncorrectAmountOfArgs(args).into());
        }
    };
    let mut file_path = current_dir()?;
    file_path.push(file);
    process_all_transactions(file_path)?;
    Ok(())
}
