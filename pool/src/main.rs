use bob_miner_v2::MinerSettings;
use bob_pool::guard::GuardPrincipal;
use bob_pool::memory::{add_member_total_cycles, get_miner_canister, set_miner_canister};
use bob_pool::{
    fetch_block, notify_top_up, MemberCycles, MAINNET_BOB_CANISTER_ID,
    MAINNET_CYCLE_MINTER_CANISTER_ID, MAINNET_LEDGER_CANISTER_ID, MAINNET_LEDGER_INDEX_CANISTER_ID,
};
use candid::{Nat, Principal};
use ic_cdk::api::call::{accept_message, arg_data_raw_size, method_name};
use ic_cdk::{init, inspect_message, query, trap, update};
use ic_ledger_types::TransferResult;
use icp_ledger::{AccountIdentifier, Memo, Operation, Subaccount, Tokens, TransferArgs};
use std::time::Duration;

fn main() {}

#[inspect_message]
fn inspect_message() {
    let method = method_name();
    if method == "join_pool" || method == "set_member_block_cycles" {
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

async fn transfer_topup_bob(amount: u64) -> Result<u64, String> {
    let sub = Subaccount::from(&ic_types::PrincipalId(MAINNET_BOB_CANISTER_ID));
    let to = AccountIdentifier::new(
        ic_types::PrincipalId(MAINNET_CYCLE_MINTER_CANISTER_ID),
        Some(sub),
    );
    let transfer_args = TransferArgs {
        memo: Memo(1347768404),
        amount: Tokens::from_e8s(amount),
        from_subaccount: None,
        fee: Tokens::from_e8s(10_000),
        to: to.to_address(),
        created_at_time: None,
    };
    let block_index = ic_cdk::call::<_, (TransferResult,)>(
        MAINNET_LEDGER_CANISTER_ID,
        "transfer",
        (transfer_args,),
    )
    .await
    .map_err(|(code, msg)| {
        format!(
            "Error while calling ICP ledger canister ({:?}): {}",
            code, msg
        )
    })?
    .0
    .map_err(|err| format!("Error from ICP ledger canister: {}", err))?;
    ic_cdk::print(format!(
        "Sent BoB top up transfer at block index {}.",
        block_index
    ));
    let get_blocks_args = icrc_ledger_types::icrc3::blocks::GetBlocksRequest {
        start: block_index.into(),
        length: Nat::from(1_u8),
    };
    loop {
        let blocks_raw = ic_cdk::call::<_, (ic_icp_index::GetBlocksResponse,)>(
            MAINNET_LEDGER_INDEX_CANISTER_ID,
            "get_blocks",
            (get_blocks_args.clone(),),
        )
        .await
        .map_err(|(code, msg)| {
            format!(
                "Error while calling ICP index canister ({:?}): {}",
                code, msg
            )
        })?
        .0;
        if blocks_raw.blocks.first().is_some() {
            break;
        }
    }
    Ok(block_index)
}

async fn spawn_miner(block_index: u64) -> Result<Principal, String> {
    ic_cdk::call::<_, (Result<Principal, String>,)>(
        MAINNET_BOB_CANISTER_ID,
        "spawn_miner",
        (block_index,),
    )
    .await
    .map_err(|(code, msg)| format!("Error while calling BoB canister ({:?}): {}", code, msg))?
    .0
    .map_err(|err| format!("Error from BoB canister: {}", err))
}

async fn update_miner_block_cycles(block_cycles: u128) -> Result<(), String> {
    let miner_id = get_miner_canister().ok_or("Miner canister not set".to_string())?;
    let update_miner_settings_args = MinerSettings {
        max_cycles_per_round: Some(block_cycles),
        new_owner: None,
    };
    ic_cdk::call::<_, ((),)>(
        miner_id,
        "update_miner_settings",
        (update_miner_settings_args,),
    )
    .await
    .map(|res| res.0)
    .map_err(|(code, msg)| format!("Error while calling miner ({:?}): {}", code, msg))
}

#[init]
fn init() {
    ic_cdk_timers::set_timer(Duration::from_secs(0), move || {
        ic_cdk::spawn(async move {
            let block_index = transfer_topup_bob(100_000_000)
                .await
                .unwrap_or_else(|err| trap(&format!("Could not top up BoB: {}", err)));
            let bob_miner_id = spawn_miner(block_index)
                .await
                .unwrap_or_else(|err| trap(&format!("Could not spawn miner: {}", err)));
            set_miner_canister(bob_miner_id);
            update_miner_block_cycles(0)
                .await
                .unwrap_or_else(|err| trap(&format!("Could not update miner settings: {}", err)));
        })
    });
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
    Ok(bob_pool::memory::get_member_cycles(ic_cdk::caller()))
}

#[update]
fn set_member_block_cycles(block_cycles: Nat) -> Result<(), String> {
    ensure_ready()?;
    let caller = ic_cdk::caller();
    if bob_pool::memory::get_member_cycles(caller).is_none() {
        return Err(format!("The caller {} is no pool member.", caller));
    }
    if block_cycles.clone() % 1_000_000_u64 != 0_u64 {
        return Err(format!(
            "The number of block cycles {} is not a multiple of 1_000_000.",
            block_cycles
        ));
    }
    bob_pool::memory::set_member_block_cycles(caller, block_cycles);
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
        let min_amount = icp_ledger::Tokens::from_e8s(99_990_000_u64);
        if amount < min_amount {
            return Err(format!(
                "Transaction amount ({}) too low: should be at least {}.",
                amount, min_amount
            ));
        }

        let res = notify_top_up(block_index).await?;
        add_member_total_cycles(caller, res.get());

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
