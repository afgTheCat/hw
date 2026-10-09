use std::env::current_dir;

use hw::{HwErrors::IncorrectAmountOfArgs, PaymentProcessor, get_transactions};

fn main() -> anyhow::Result<()> {
    let input: Vec<_> = std::env::args().collect();
    let file = match input.as_slice() {
        [_, input] => input.clone(),
        [_, args @ ..] | [args @ ..] => {
            let args = args.to_vec();
            return Err(IncorrectAmountOfArgs(args).into());
        }
    };
    let mut file_path = current_dir()?;
    file_path.push(file);
    let transactions = get_transactions(file_path)?;
    let mut payment_processor = PaymentProcessor::default();
    payment_processor.process_all_transactions(transactions)?;
    payment_processor.report()?;
    Ok(())
}
