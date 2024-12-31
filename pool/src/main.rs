use bob_miner_v2::{MinerSettings, StatsV2};
use bob_minter_v2::Stats;
use bob_pool::guard::GuardPrincipal;
use bob_pool::guard::{TaskGuard, TaskType};
use bob_pool::memory::{
    add_member_total_cycles, add_rewards, commit_block_participants, get_miner_canister,
    get_next_block_participants, get_rewards, set_miner_canister, set_rewards,
    total_pending_rewards,
};
use bob_pool::{
    fetch_block, notify_top_up, MemberCycles, MAINNET_BOB_CANISTER_ID,
    MAINNET_BOB_LEDGER_CANISTER_ID, MAINNET_CYCLE_MINTER_CANISTER_ID, MAINNET_LEDGER_CANISTER_ID,
    MAINNET_LEDGER_INDEX_CANISTER_ID,
};
use candid::{Nat, Principal};
use ic_cdk::api::call::{accept_message, arg_data_raw_size, method_name};
use ic_cdk::api::canister_balance128;
use ic_cdk::api::management_canister::main::{deposit_cycles, CanisterIdRecord};
use ic_cdk::{init, inspect_message, post_upgrade, query, trap, update};
use ic_ledger_types::TransferResult;
use icp_ledger::{AccountIdentifier, Memo, Operation, Subaccount, Tokens, TransferArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{TransferArg, TransferError};
use std::collections::BTreeSet;
use std::future::Future;
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

async fn get_bob_balance() -> Result<u128, String> {
    ic_cdk::call::<_, (Nat,)>(
        MAINNET_BOB_LEDGER_CANISTER_ID,
        "icrc1_balance_of",
        (Account {
            owner: ic_cdk::id(),
            subaccount: None,
        },),
    )
    .await
    .map(|res| res.0 .0.try_into().unwrap())
    .map_err(|(code, msg)| {
        format!(
            "Error while calling BoB ledger canister ({:?}): {}",
            code, msg
        )
    })
}

async fn transfer_bob(user_id: Principal, amount: u128) -> Result<u64, String> {
    ic_cdk::call::<_, (Result<Nat, TransferError>,)>(
        MAINNET_BOB_LEDGER_CANISTER_ID,
        "icrc1_transfer",
        (TransferArg {
            from_subaccount: None,
            to: Account {
                owner: user_id,
                subaccount: None,
            },
            fee: Some(1_000_000_u64.into()),
            created_at_time: None,
            memo: None,
            amount: amount.into(),
        },),
    )
    .await
    .map(|res| {
        res.0
            .map(|block| block.0.try_into().unwrap())
            .map_err(|err| format!("Error from BoB ledger canister: {}", err))
    })
    .map_err(|(code, msg)| {
        format!(
            "Error while calling BoB ledger canister ({:?}): {}",
            code, msg
        )
    })?
}

async fn get_bob_stats() -> Result<Stats, String> {
    ic_cdk::call::<_, (Stats,)>(MAINNET_BOB_CANISTER_ID, "get_statistics", ((),))
        .await
        .map(|res| res.0)
        .map_err(|(code, msg)| format!("Error while calling BoB canister ({:?}): {}", code, msg))
}

async fn get_miner_stats() -> Result<StatsV2, String> {
    let miner = get_miner().unwrap();
    ic_cdk::call::<_, (StatsV2,)>(miner, "get_statistics_v2", ((),))
        .await
        .map(|res| res.0)
        .map_err(|(code, msg)| format!("Error while calling miner canister ({:?}): {}", code, msg))
}

async fn upgrade_miner() -> Result<(), String> {
    let miner = get_miner().unwrap();
    ic_cdk::call::<_, (Result<(), String>,)>(MAINNET_BOB_CANISTER_ID, "upgrade_miner", (miner,))
        .await
        .map(|res| res.0)
        .map_err(|(code, msg)| format!("Error while calling BoB canister ({:?}): {}", code, msg))?
}

fn retry_and_log<F, A, Fut>(
    initial_delay: Duration,
    retry_delay: Duration,
    max_attempts: u64,
    phase: &'static str,
    f: F,
    arg: A,
) where
    F: FnOnce(A) -> Fut + Copy + 'static,
    A: Copy + 'static,
    Fut: Future<Output = Result<(), String>>,
{
    ic_cdk_timers::set_timer(initial_delay, move || {
        ic_cdk::spawn(async move {
            if let Err(err) = f(arg).await {
                ic_cdk::print(format!("ERR({}): {}", phase, err));
                if max_attempts == 0 {
                    ic_cdk::print(format!(
                        "ERR(retry_and_log): Exceeded max attempts in {}: starting from scratch.",
                        phase
                    ));
                    run();
                } else {
                    retry_and_log(retry_delay, retry_delay, max_attempts - 1, phase, f, arg);
                }
            }
        });
    });
}

async fn pay_rewards(idx: u64) -> Result<(), String> {
    let _guard_principal = TaskGuard::new(TaskType::PayRewards)
        .map_err(|guard_error| format!("Concurrency error: {:?}", guard_error))?;
    let mut rewards = get_rewards(idx);
    let done: BTreeSet<_> = rewards
        .transfer_idx
        .iter()
        .map(|(member, _)| member)
        .cloned()
        .collect();
    for (member, amount) in rewards.participants.clone().into_iter() {
        if !done.contains(&member) {
            let block_idx = transfer_bob(member, amount).await?;
            rewards.pending = rewards.pending.checked_sub(amount + 1_000_000).unwrap();
            rewards.transfer_idx.push((member, block_idx));
            set_rewards(idx, rewards.clone());
        }
    }
    Ok(())
}

async fn check_rewards() -> Result<(), String> {
    let _guard_principal = TaskGuard::new(TaskType::CheckRewards)
        .map_err(|guard_error| format!("Concurrency error: {:?}", guard_error))?;
    let bob_balance = get_bob_balance()
        .await?
        .checked_sub(total_pending_rewards())
        .unwrap();
    if bob_balance != 0_u128 {
        let rewards_idx = add_rewards(bob_balance);
        retry_and_log(
            Duration::from_secs(0),
            Duration::from_secs(600),
            100,
            "rewards",
            pay_rewards,
            rewards_idx,
        );
    }
    Ok(())
}

fn run() {
    retry_and_log(
        Duration::from_secs(0),
        Duration::from_secs(30),
        10,
        "schedule_stage_1",
        schedule_stage_1,
        (),
    );
}

async fn schedule_stage_1(_: ()) -> Result<(), String> {
    update_miner_block_cycles(0).await?;
    check_rewards().await?;
    let stats = get_bob_stats().await?;
    let time_since_last_block = stats.time_since_last_block;
    if time_since_last_block >= 490 {
        let block_count = stats.block_count;
        return Err(format!(
            "Time since last block {} too high: {}",
            block_count, time_since_last_block
        ));
    }
    retry_and_log(
        Duration::from_secs(490 - time_since_last_block),
        Duration::from_secs(0),
        1,
        "stage_1",
        stage_1,
        (),
    );
    Ok(())
}

async fn stage_1(_: ()) -> Result<(), String> {
    check_rewards().await?;
    let next_block_participants = get_next_block_participants();
    let total_member_block_cycles = next_block_participants
        .iter()
        .map(|(_, block_cycles)| block_cycles)
        .sum();
    if total_member_block_cycles == 0 {
        run();
        return Ok(());
    }
    upgrade_miner().await?;
    update_miner_block_cycles(total_member_block_cycles).await?;
    let miner_stats = get_miner_stats().await?;
    let target_miner_cycle_balance = total_member_block_cycles + 1_000_000_000_000;
    let top_up_cycles = target_miner_cycle_balance.saturating_sub(miner_stats.cycle_balance.into());
    if canister_balance128() - top_up_cycles < 1_000_000_000_000 {
        trap(&format!(
            "Pool cycles {} too low after topping up miner with {} cycles.",
            canister_balance128(),
            top_up_cycles
        ));
    }
    let miner = get_miner().unwrap();
    deposit_cycles(CanisterIdRecord { canister_id: miner }, top_up_cycles)
        .await
        .map_err(|(code, msg)| {
            format!(
                "Error while depositing cycles to miner ({:?}): {}",
                code, msg
            )
        })?;
    commit_block_participants(next_block_participants);
    retry_and_log(
        Duration::from_secs(250),
        Duration::from_secs(10),
        3,
        "stage_2",
        stage_2,
        total_member_block_cycles,
    );
    Ok(())
}

async fn stage_2(total_member_block_cycles: u128) -> Result<(), String> {
    let miner_stats = get_miner_stats().await?;
    if miner_stats.last_round_cyles_burned != total_member_block_cycles {
        return Err(format!(
            "Last cycles burned {} do not match the expectation {}.",
            miner_stats.last_round_cyles_burned, total_member_block_cycles
        ));
    }
    run();
    Ok(())
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
    let miner_id = get_miner_canister().unwrap();
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
            run();
        })
    });
}

#[post_upgrade]
async fn post_upgrade() {
    if get_miner().is_none() {
        trap("No miner found.");
    }
    run();
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
    if block_cycles.clone() != 0_u64 && block_cycles.clone() < 15_000_000_000_u64 {
        return Err(format!(
            "The number of block cycles {} is too small.",
            block_cycles
        ));
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
