use payments_engine::{PaymentError, PaymentProcessor, Transaction};

// All amounts are integer units of 0.0001, matching Transaction's public API.
fn process(transactions: Vec<Transaction>) -> PaymentProcessor {
    let mut processor = PaymentProcessor::default();
    processor.process_all_transactions(transactions).unwrap();
    processor
}

fn funded_processor() -> PaymentProcessor {
    process(vec![Transaction::deposit(1, 1, 100).unwrap()])
}

fn assert_client(processor: &PaymentProcessor, id: u16, available: i128, held: i64, locked: bool) {
    let client = processor.client(id).expect("client should exist");
    assert_eq!(
        i128::from(client.available),
        available,
        "client {id}: available"
    );
    assert_eq!(client.held, held, "client {id}: held");
    assert_eq!(client.locked, locked, "client {id}: locked");
}

#[test]
fn sample_transactions_produce_expected_balances() {
    let processor = process(vec![
        Transaction::deposit(1, 1, 10_000).unwrap(),
        Transaction::deposit(2, 2, 20_000).unwrap(),
        Transaction::deposit(1, 3, 20_000).unwrap(),
        Transaction::withdrawal(1, 4, 15_000).unwrap(),
        Transaction::withdrawal(2, 5, 30_000).unwrap(),
    ]);
    assert_client(&processor, 1, 15_000, 0, false);
    assert_client(&processor, 2, 20_000, 0, false);
}

#[test]
fn smallest_amounts_are_preserved_exactly() {
    let processor = process(vec![
        Transaction::deposit(1, 1, 1).unwrap(),
        Transaction::deposit(1, 2, 2).unwrap(),
        Transaction::withdrawal(1, 3, 1).unwrap(),
        Transaction::dispute(1, 2),
    ]);
    assert_client(&processor, 1, 0, 2, false);
}

#[test]
fn withdrawal_can_spend_exactly_the_available_balance() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![Transaction::withdrawal(1, 2, 100).unwrap()])
        .unwrap();
    assert_client(&processor, 1, 0, 0, false);
}

#[test]
fn withdrawal_cannot_spend_held_funds() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::deposit(1, 2, 40).unwrap(),
            Transaction::dispute(1, 1),
            Transaction::withdrawal(1, 3, 41).unwrap(),
        ])
        .unwrap();
    assert_client(&processor, 1, 40, 100, false);
    processor
        .process_all_transactions(vec![Transaction::withdrawal(1, 4, 40).unwrap()])
        .unwrap();
    assert_client(&processor, 1, 0, 100, false);
}

#[test]
fn rejected_withdrawal_creates_an_empty_client() {
    let processor = process(vec![Transaction::withdrawal(42, 1, 1).unwrap()]);
    assert_client(&processor, 42, 0, 0, false);
    assert!(processor.client(1).is_none());
}

#[test]
fn unknown_references_create_clients_without_changing_balances() {
    let processor = process(vec![
        Transaction::dispute(1, 99),
        Transaction::resolve(2, 99),
        Transaction::chargeback(3, 99),
    ]);
    for id in 1..=3 {
        assert_client(&processor, id, 0, 0, false);
    }
}

#[test]
fn unknown_dispute_is_not_applied_to_a_later_deposit() {
    let processor = process(vec![
        Transaction::dispute(1, 1),
        Transaction::deposit(1, 1, 100).unwrap(),
        Transaction::resolve(1, 1),
        Transaction::chargeback(1, 1),
    ]);
    assert_client(&processor, 1, 100, 0, false);
}

#[test]
fn resolve_and_chargeback_require_a_pending_dispute() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::resolve(1, 1),
            Transaction::chargeback(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, 100, 0, false);
    processor
        .process_all_transactions(vec![Transaction::dispute(1, 1)])
        .unwrap();
    assert_client(&processor, 1, 0, 100, false);
}

#[test]
fn wrong_client_cannot_dispute_or_block_the_owners_dispute() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::deposit(2, 2, 200).unwrap(),
            Transaction::dispute(2, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, 100, 0, false);
    assert_client(&processor, 2, 200, 0, false);
    processor
        .process_all_transactions(vec![Transaction::dispute(1, 1)])
        .unwrap();
    assert_client(&processor, 1, 0, 100, false);
    assert_client(&processor, 2, 200, 0, false);
}

#[test]
fn wrong_client_cannot_resolve_or_charge_back_another_clients_dispute() {
    for action in [Transaction::resolve(2, 1), Transaction::chargeback(2, 1)] {
        let mut processor = funded_processor();
        processor
            .process_all_transactions(vec![
                Transaction::deposit(2, 2, 200).unwrap(),
                Transaction::dispute(1, 1),
                Transaction::dispute(2, 2),
                action,
            ])
            .unwrap();
        assert_client(&processor, 1, 0, 100, false);
        assert_client(&processor, 2, 0, 200, false);
        processor
            .process_all_transactions(vec![Transaction::resolve(1, 1)])
            .unwrap();
        assert_client(&processor, 1, 100, 0, false);
        assert_client(&processor, 2, 0, 200, false);
    }
}

#[test]
fn successful_and_rejected_withdrawals_cannot_be_disputed() {
    for (amount, remaining) in [(40, 60), (101, 100)] {
        let mut processor = funded_processor();
        processor
            .process_all_transactions(vec![
                Transaction::withdrawal(1, 2, amount).unwrap(),
                Transaction::dispute(1, 2),
                Transaction::resolve(1, 2),
                Transaction::dispute(1, 2),
                Transaction::chargeback(1, 2),
            ])
            .unwrap();
        assert_client(&processor, 1, remaining, 0, false);
    }
}

#[test]
fn spent_deposit_can_be_disputed_and_resolved() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::withdrawal(1, 2, 80).unwrap(),
            Transaction::dispute(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, -80, 100, false);
    processor
        .process_all_transactions(vec![Transaction::resolve(1, 1)])
        .unwrap();
    assert_client(&processor, 1, 20, 0, false);
}

#[test]
fn spent_deposit_can_be_charged_back_leaving_a_negative_total() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::withdrawal(1, 2, 100).unwrap(),
            Transaction::dispute(1, 1),
            Transaction::chargeback(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, -100, 0, true);
}

#[test]
fn negative_available_balance_blocks_withdrawals_but_accepts_deposits() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::withdrawal(1, 2, 80).unwrap(),
            Transaction::dispute(1, 1),
            Transaction::withdrawal(1, 3, 1).unwrap(),
        ])
        .unwrap();
    assert_client(&processor, 1, -80, 100, false);
    processor
        .process_all_transactions(vec![Transaction::deposit(1, 4, 90).unwrap()])
        .unwrap();
    assert_client(&processor, 1, 10, 100, false);
    processor
        .process_all_transactions(vec![Transaction::withdrawal(1, 5, 10).unwrap()])
        .unwrap();
    assert_client(&processor, 1, 0, 100, false);
}

#[test]
fn duplicate_pending_disputes_hold_funds_only_once() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::dispute(1, 1),
            Transaction::dispute(1, 1),
            Transaction::dispute(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, 0, 100, false);
    processor
        .process_all_transactions(vec![Transaction::resolve(1, 1)])
        .unwrap();
    assert_client(&processor, 1, 100, 0, false);
}

#[test]
fn resolved_disputes_ignore_duplicate_resolutions_and_chargebacks() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::dispute(1, 1),
            Transaction::resolve(1, 1),
            Transaction::resolve(1, 1),
            Transaction::chargeback(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, 100, 0, false);
}

#[test]
fn deposit_can_be_disputed_repeatedly_after_resolution_then_charged_back() {
    let mut processor = funded_processor();
    for _ in 0..3 {
        processor
            .process_all_transactions(vec![Transaction::dispute(1, 1)])
            .unwrap();
        assert_client(&processor, 1, 0, 100, false);
        processor
            .process_all_transactions(vec![Transaction::resolve(1, 1)])
            .unwrap();
        assert_client(&processor, 1, 100, 0, false);
    }
    processor
        .process_all_transactions(vec![
            Transaction::dispute(1, 1),
            Transaction::chargeback(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, 0, 0, true);
}

#[test]
fn chargeback_is_final_even_if_other_funds_are_held() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::deposit(1, 2, 200).unwrap(),
            Transaction::dispute(1, 1),
            Transaction::dispute(1, 2),
            Transaction::chargeback(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, 0, 200, true);
    processor
        .process_all_transactions(vec![
            Transaction::chargeback(1, 1),
            Transaction::resolve(1, 1),
            Transaction::dispute(1, 1),
            Transaction::resolve(1, 1),
            Transaction::chargeback(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, 0, 200, true);
}

#[test]
fn simultaneous_disputes_are_settled_independently() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::deposit(1, 2, 40).unwrap(),
            Transaction::dispute(1, 1),
            Transaction::dispute(1, 2),
        ])
        .unwrap();
    assert_client(&processor, 1, 0, 140, false);
    processor
        .process_all_transactions(vec![Transaction::resolve(1, 2)])
        .unwrap();
    assert_client(&processor, 1, 40, 100, false);
    processor
        .process_all_transactions(vec![Transaction::chargeback(1, 1)])
        .unwrap();
    assert_client(&processor, 1, 40, 0, true);
}

#[test]
fn frozen_account_rejects_deposits_and_withdrawals() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::deposit(1, 2, 200).unwrap(),
            Transaction::dispute(1, 1),
            Transaction::chargeback(1, 1),
        ])
        .unwrap();
    processor
        .process_all_transactions(vec![Transaction::deposit(1, 3, 50).unwrap()])
        .unwrap();
    assert_client(&processor, 1, 200, 0, true);
    processor
        .process_all_transactions(vec![Transaction::withdrawal(1, 4, 50).unwrap()])
        .unwrap();
    assert_client(&processor, 1, 200, 0, true);
    processor
        .process_all_transactions(vec![Transaction::dispute(1, 3)])
        .unwrap();
    assert_client(&processor, 1, 200, 0, true);
}

#[test]
fn freezing_one_client_does_not_freeze_other_clients() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::dispute(1, 1),
            Transaction::chargeback(1, 1),
            Transaction::deposit(2, 2, 200).unwrap(),
            Transaction::withdrawal(2, 3, 50).unwrap(),
        ])
        .unwrap();
    assert_client(&processor, 1, 0, 0, true);
    assert_client(&processor, 2, 150, 0, false);
}

#[test]
fn frozen_account_can_resolve_a_dispute_that_was_already_pending() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::deposit(1, 2, 200).unwrap(),
            Transaction::dispute(1, 1),
            Transaction::dispute(1, 2),
            Transaction::chargeback(1, 1),
            Transaction::resolve(1, 2),
        ])
        .unwrap();
    assert_client(&processor, 1, 200, 0, true);
}

#[test]
fn frozen_account_accepts_new_and_reopened_disputes_and_their_settlements() {
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::deposit(1, 2, 200).unwrap(),
            Transaction::dispute(1, 1),
            Transaction::chargeback(1, 1),
            Transaction::dispute(1, 2),
        ])
        .unwrap();
    assert_client(&processor, 1, 0, 200, true);
    processor
        .process_all_transactions(vec![Transaction::resolve(1, 2)])
        .unwrap();
    assert_client(&processor, 1, 200, 0, true);
    processor
        .process_all_transactions(vec![Transaction::dispute(1, 2)])
        .unwrap();
    assert_client(&processor, 1, 0, 200, true);
    processor
        .process_all_transactions(vec![Transaction::chargeback(1, 2)])
        .unwrap();
    assert_client(&processor, 1, 0, 0, true);
}

#[test]
fn client_and_transaction_ids_accept_boundaries_and_nonascending_order() {
    let processor = process(vec![
        Transaction::deposit(u16::MAX, u32::MAX, 100).unwrap(),
        Transaction::deposit(0, 0, 200).unwrap(),
        Transaction::withdrawal(u16::MAX, 42, 50).unwrap(),
        Transaction::dispute(0, 0),
        Transaction::dispute(u16::MAX, u32::MAX),
        Transaction::resolve(u16::MAX, u32::MAX),
        Transaction::chargeback(0, 0),
    ]);
    assert_client(&processor, u16::MAX, 50, 0, false);
    assert_client(&processor, 0, 0, 0, true);
}

#[test]
fn zero_amount_deposit_still_has_a_dispute_lifecycle() {
    let mut processor = process(vec![
        Transaction::deposit(1, 1, 0).unwrap(),
        Transaction::withdrawal(1, 2, 0).unwrap(),
        Transaction::dispute(1, 1),
        Transaction::resolve(1, 1),
    ]);
    assert_client(&processor, 1, 0, 0, false);
    processor
        .process_all_transactions(vec![
            Transaction::dispute(1, 1),
            Transaction::chargeback(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, 0, 0, true);
}

#[test]
fn largest_signed_amount_can_be_spent_disputed_and_resolved() {
    let amount = i64::MAX;
    let mut processor = process(vec![
        Transaction::deposit(1, 1, amount).unwrap(),
        Transaction::withdrawal(1, 2, amount).unwrap(),
        Transaction::dispute(1, 1),
    ]);
    assert_client(&processor, 1, -i128::from(amount), amount, false);
    processor
        .process_all_transactions(vec![Transaction::resolve(1, 1)])
        .unwrap();
    assert_client(&processor, 1, 0, 0, false);
}

#[test]
fn negative_deposit_is_rejected() {
    for amount in [-1, i64::MIN] {
        let result = Transaction::deposit(1, 1, amount);
        assert!(matches!(result, Err(PaymentError::InvalidAmount { .. })));
    }
}

#[test]
fn negative_withdrawal_is_rejected() {
    for amount in [-1, i64::MIN] {
        let result = Transaction::withdrawal(1, 1, amount);
        assert!(matches!(result, Err(PaymentError::InvalidAmount { .. })));
    }
}

#[test]
fn balance_overflow_stops_the_batch_and_preserves_prior_transactions() {
    let amount = i64::MAX;
    let mut processor = PaymentProcessor::default();
    let result = processor.process_all_transactions(vec![
        Transaction::deposit(1, 1, amount).unwrap(),
        Transaction::deposit(1, 2, 1).unwrap(),
        Transaction::deposit(2, 3, 1).unwrap(),
    ]);
    assert!(matches!(result, Err(PaymentError::ArithmeticError)));
    assert_client(&processor, 1, i128::from(amount), 0, false);
    assert!(processor.client(2).is_none());
    processor
        .process_all_transactions(vec![Transaction::dispute(1, 2)])
        .unwrap();
    assert_client(&processor, 1, i128::from(amount), 0, false);
}

#[test]
fn dispute_underflow_preserves_balances_and_allows_retry() {
    let amount = i64::MAX;
    for reopen in [false, true] {
        let mut processor = process(vec![
            Transaction::deposit(1, 1, amount).unwrap(),
            Transaction::withdrawal(1, 2, amount).unwrap(),
            Transaction::deposit(1, 3, amount).unwrap(),
            Transaction::withdrawal(1, 4, amount).unwrap(),
        ]);
        if reopen {
            processor
                .process_all_transactions(vec![
                    Transaction::dispute(1, 3),
                    Transaction::resolve(1, 3),
                ])
                .unwrap();
        }
        processor
            .process_all_transactions(vec![Transaction::dispute(1, 1)])
            .unwrap();
        let result = processor.process_all_transactions(vec![Transaction::dispute(1, 3)]);
        assert!(matches!(result, Err(PaymentError::ArithmeticError)));
        assert_client(&processor, 1, -i128::from(amount), amount, false);

        processor
            .process_all_transactions(vec![Transaction::resolve(1, 1), Transaction::dispute(1, 3)])
            .unwrap();
        assert_client(&processor, 1, -i128::from(amount), amount, false);
    }
}

#[test]
fn held_overflow_does_not_partially_apply_a_dispute() {
    let amount = i64::MAX / 2;
    let mut processor = process(vec![
        Transaction::deposit(1, 1, amount).unwrap(),
        Transaction::dispute(1, 1),
        Transaction::deposit(1, 2, amount).unwrap(),
        Transaction::dispute(1, 2),
        Transaction::deposit(1, 3, 2).unwrap(),
    ]);
    let result = processor.process_all_transactions(vec![Transaction::dispute(1, 3)]);
    assert!(matches!(result, Err(PaymentError::ArithmeticError)));
    assert_client(&processor, 1, 2, 2 * amount, false);

    processor
        .process_all_transactions(vec![
            Transaction::withdrawal(1, 4, 2).unwrap(),
            Transaction::resolve(1, 1),
            Transaction::dispute(1, 3),
        ])
        .unwrap();
    assert_client(&processor, 1, i128::from(amount) - 2, amount + 2, false);
}

#[test]
fn resolve_overflow_preserves_held_funds_and_pending_status() {
    let amount = i64::MAX;
    let mut processor = funded_processor();
    processor
        .process_all_transactions(vec![
            Transaction::dispute(1, 1),
            Transaction::deposit(1, 2, amount).unwrap(),
        ])
        .unwrap();

    let result = processor.process_all_transactions(vec![Transaction::resolve(1, 1)]);
    assert!(matches!(result, Err(PaymentError::ArithmeticError)));
    assert_client(&processor, 1, i128::from(amount), 100, false);

    processor
        .process_all_transactions(vec![
            Transaction::withdrawal(1, 3, 100).unwrap(),
            Transaction::resolve(1, 1),
        ])
        .unwrap();
    assert_client(&processor, 1, i128::from(amount), 0, false);
}
