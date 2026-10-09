use std::env::current_dir;

use payments_engine::{PaymentError::IncorrectAmountOfArgs, process_csv_file};

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
    process_csv_file(file_path)?;
    Ok(())
}
