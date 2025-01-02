use bob_pool::{
    add_member_remaining_cycles, fetch_block, get_miner_canister, init_member_rewards,
    notify_top_up, pay_rewards, run, set_miner_canister, spawn_miner, transfer, GuardPrincipal,
    MemberCycles, Reward, MAINNET_BOB_CANISTER_ID, MAINNET_CYCLE_MINTER_CANISTER_ID,
};
use candid::Principal;
use ic_cdk::api::call::{accept_message, arg_data_raw_size, method_name};
use ic_cdk::{init, inspect_message, post_upgrade, query, trap, update};
use icp_ledger::{AccountIdentifier, Operation, Subaccount};
use std::time::Duration;

fn main() {}

#[inspect_message]
fn inspect_message() {
    let method = method_name();
    if method == "join_pool"
        || method == "pay_member_rewards"
        || method == "set_member_block_cycles"
    {
        let arg_size = arg_data_raw_size();
        if arg_size > 1_000 {
            trap(&format!(
                "Unexpected argument length of {} for method {}.",
                arg_size, method
            ))
        } else {
            accept_message();
        }
    } else {
        trap(&format!(
            "The method {} cannot be called via ingress messages.",
            method
        ));
    }
}

async fn do_init() -> Result<Principal, String> {
    let block_index = transfer(
        MAINNET_CYCLE_MINTER_CANISTER_ID,
        Some(MAINNET_BOB_CANISTER_ID),
        1347768404,
        100_000_000, // minimum amount of 1ICP (surplus discarded)
    )
    .await?;
    ic_cdk::print(format!(
        "Sent BoB top up transfer at ICP ledger block index {}.",
        block_index
    ));

    while fetch_block(block_index).await.is_err() {}

    let miner = spawn_miner(block_index).await?;

    Ok(miner)
}

#[init]
fn init() {
    ic_cdk_timers::set_timer(Duration::from_secs(0), move || {
        ic_cdk::spawn(async move {
            let miner = do_init()
                .await
                .unwrap_or_else(|err| trap(&format!("Failed to init: {}", err)));
            set_miner_canister(miner);
            run(Duration::from_secs(0));
        })
    });
}

#[post_upgrade]
fn post_upgrade() {
    if get_miner().is_none() {
        trap("No miner found.");
    }
    run(Duration::from_secs(0));
}

#[query]
fn get_member_rewards() -> Vec<Reward> {
    bob_pool::get_member_rewards(ic_cdk::caller())
}

#[update]
async fn pay_member_rewards() -> Result<(), String> {
    ensure_ready()?;
    pay_rewards(ic_cdk::caller()).await
}

#[query]
fn get_miner() -> Option<Principal> {
    get_miner_canister()
}

fn ensure_ready() -> Result<(), String> {
    get_miner_canister()
        .map(|_| ())
        .ok_or("BoB pool canister is not ready; please try again later.".to_string())
}

#[query]
fn get_member_cycles() -> Result<Option<MemberCycles>, String> {
    ensure_ready()?;
    Ok(bob_pool::get_member_cycles(ic_cdk::caller()))
}

#[update]
fn set_member_block_cycles(block_cycles: u128) -> Result<(), String> {
    ensure_ready()?;
    let caller = ic_cdk::caller();
    if bob_pool::get_member_cycles(caller).is_none() {
        return Err(format!("The caller {} is no pool member.", caller));
    }
    if block_cycles != 0 && block_cycles < 15_000_000_000 {
        return Err(format!(
            "The number of block cycles {} is too small.",
            block_cycles
        ));
    }
    if block_cycles % 1_000_000 != 0 {
        return Err(format!(
            "The number of block cycles {} is not a multiple of {}.",
            block_cycles, 1000000_u128
        ));
    }
    bob_pool::set_member_block_cycles(caller, block_cycles);
    Ok(())
}

#[update]
async fn join_pool(block_index: u64) -> Result<(), String> {
    ensure_ready()?;
    let caller = ic_cdk::caller();
    if caller == Principal::anonymous() {
        return Err("Anonymous principal cannot join pool.".to_string());
    }
    let _guard_principal = GuardPrincipal::new(caller)
        .map_err(|guard_error| format!("Concurrency error: {:?}", guard_error))?;

    let transaction = fetch_block(block_index).await?.transaction;

    let expected_memo = 1347768404;
    if transaction.memo != icp_ledger::Memo(expected_memo) {
        return Err(format!(
            "Invalid memo ({}): should be {}.",
            transaction.memo.0, expected_memo
        ));
    }

    if let Operation::Transfer {
        from, to, amount, ..
    } = transaction.operation
    {
        let expect_from = AccountIdentifier::new(ic_types::PrincipalId(caller), None);
        if from != expect_from {
            return Err(format!(
                "Unexpected sender account ({}): should be {}.",
                from, expect_from
            ));
        }
        let sub = Subaccount::from(&ic_types::PrincipalId(ic_cdk::id()));
        let expect_to = AccountIdentifier::new(
            ic_types::PrincipalId(MAINNET_CYCLE_MINTER_CANISTER_ID),
            Some(sub),
        );
        if to != expect_to {
            return Err(format!(
                "Unexpected destination account ({}): should be {}.",
                to, expect_to
            ));
        }
        let min_amount = icp_ledger::Tokens::from_e8s(99_990_000);
        if amount < min_amount {
            return Err(format!(
                "Transaction amount ({}) too low: should be at least {}.",
                amount, min_amount
            ));
        }

        let res = notify_top_up(block_index).await?;
        add_member_remaining_cycles(caller, res.get());
        init_member_rewards(caller);

        Ok(())
    } else {
        Err("Unexpected transaction operation: should be transfer.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid_parser::utils::{service_equal, CandidSource};

    #[test]
    fn test_implemented_interface_matches_declared_interface_exactly() {
        let declared_interface = include_str!("../pool.did");
        let declared_interface = CandidSource::Text(declared_interface);

        // The line below generates did types and service definition from the
        // methods annotated with Rust CDK macros above. The definition is then
        // obtained with `__export_service()`.
        candid::export_service!();
        let implemented_interface_str = __export_service();
        let implemented_interface = CandidSource::Text(&implemented_interface_str);

        let result = service_equal(declared_interface, implemented_interface);
        assert!(result.is_ok(), "{:?}\n\n", result.unwrap_err());
    }
}
