use bob_pool::{
    add_member_remaining_cycles, fetch_block, init_member_rewards, notify_top_up, pay_rewards,
    GuardPrincipal, MemberCycles, PoolState, Reward, MAINNET_CYCLE_MINTER_CANISTER_ID,
};
use candid::Principal;
use ic_cdk::api::call::{accept_message, arg_data_raw_size, method_name};
use ic_cdk::api::{in_replicated_execution, is_controller};
use ic_cdk::{inspect_message, query, trap, update};
use icp_ledger::{AccountIdentifier, Operation, Subaccount};

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
                "Unexpected argument length of {} for method `{}`.",
                arg_size, method
            ))
        } else {
            accept_message();
        }
    } else if method == "spawn_miner" || method == "start" || method == "stop" {
        if is_controller(&ic_cdk::caller()) {
            accept_message();
        } else {
            trap(&format!(
                "The method `{}` can only be called by controllers.",
                method
            ));
        }
    } else {
        trap(&format!(
            "The method `{}` cannot be called via ingress messages.",
            method
        ));
    }
}

fn ensure_caller_pool_member() -> Result<(), String> {
    let caller = ic_cdk::caller();
    if bob_pool::get_member_cycles(caller).is_none() {
        Err(format!("The caller {} is no pool member.", caller))
    } else {
        Ok(())
    }
}

#[query]
fn get_member_cycles() -> Result<MemberCycles, String> {
    ensure_caller_pool_member()?;
    Ok(bob_pool::get_member_cycles(ic_cdk::caller()).unwrap())
}

#[query]
fn get_member_rewards() -> Result<Vec<Reward>, String> {
    ensure_caller_pool_member()?;
    Ok(bob_pool::get_member_rewards(ic_cdk::caller()))
}

#[query]
fn get_pool_state() -> Result<PoolState, String> {
    if in_replicated_execution() {
        return Err(
            "The method `get_pool_state` can only be called as non-replicated query call."
                .to_string(),
        );
    }
    Ok(bob_pool::get_pool_state())
}

#[update]
async fn pay_member_rewards() -> Result<(), String> {
    ensure_caller_pool_member()?;
    pay_rewards(ic_cdk::caller()).await
}

#[update]
fn set_member_block_cycles(block_cycles: u128) -> Result<(), String> {
    ensure_caller_pool_member()?;
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
    bob_pool::set_member_block_cycles(ic_cdk::caller(), block_cycles);
    Ok(())
}

#[update]
async fn spawn_miner(block_index: Option<u64>) -> Result<(), String> {
    if !is_controller(&ic_cdk::caller()) {
        return Err("Only controllers can call `spawn_miner`.".to_string());
    }
    bob_pool::spawn_miner(block_index).await
}

#[update]
fn start() -> Result<(), String> {
    if !is_controller(&ic_cdk::caller()) {
        return Err("Only controllers can call `start`.".to_string());
    }
    bob_pool::start()
}

#[update]
fn stop() -> Result<(), String> {
    if !is_controller(&ic_cdk::caller()) {
        return Err("Only controllers can call `stop`.".to_string());
    }
    bob_pool::stop();
    Ok(())
}

#[update]
async fn join_pool(block_index: u64) -> Result<(), String> {
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
