use bob_pool::{
    add_member_remaining_cycles, add_rewards, commit_block_participants, fetch_block,
    get_bob_statistics, get_last_reward_timestamp, get_latest_blocks, get_miner_canister,
    get_miner_statistics, get_next_block_participants, notify_top_up, pay_rewards,
    set_last_reward_timestamp, set_member_rewards, set_miner_canister, spawn_miner, transfer,
    update_miner_settings, upgrade_miner, GuardPrincipal, MemberCycles, Reward, TaskGuard,
    TaskType, MAINNET_BOB_CANISTER_ID, MAINNET_CYCLE_MINTER_CANISTER_ID,
};
use candid::Principal;
use ic_cdk::api::call::{accept_message, arg_data_raw_size, method_name};
use ic_cdk::api::canister_balance128;
use ic_cdk::api::management_canister::main::{deposit_cycles, CanisterIdRecord};
use ic_cdk::{init, inspect_message, post_upgrade, query, trap, update};
use icp_ledger::{AccountIdentifier, Operation, Subaccount};
use std::cmp::max;
use std::future::Future;
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

async fn check_rewards() -> Result<(), String> {
    let _guard_principal = TaskGuard::new(TaskType::CheckRewards)
        .map_err(|guard_error| format!("Concurrency error: {:?}", guard_error))?;
    let latest_blocks = get_latest_blocks().await?;
    let mut total_bob_rewards: u128 = 0;
    let last_reward_timestamp = get_last_reward_timestamp();
    let mut max_reward_timestamp = 0;
    for block in latest_blocks {
        if block.to == ic_cdk::id() && block.timestamp > last_reward_timestamp {
            total_bob_rewards = total_bob_rewards.checked_add(block.rewards.into()).unwrap();
            max_reward_timestamp = max(max_reward_timestamp, block.timestamp);
        }
    }
    if total_bob_rewards > 0 {
        add_rewards(total_bob_rewards);
        assert_ne!(max_reward_timestamp, 0);
        set_last_reward_timestamp(max_reward_timestamp);
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
    let miner = get_miner().unwrap();
    update_miner_settings(miner, Some(0), None).await?;
    check_rewards().await?;
    let stats = get_bob_statistics().await?;
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
    let miner = get_miner().unwrap();
    upgrade_miner(miner).await?;
    update_miner_settings(miner, Some(total_member_block_cycles), None).await?;
    let miner_stats = get_miner_statistics(miner).await?;
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
    let miner = get_miner().unwrap();
    let miner_stats = get_miner_statistics(miner).await?;
    if miner_stats.last_round_cyles_burned != total_member_block_cycles {
        return Err(format!(
            "Last cycles burned {} do not match the expectation {}.",
            miner_stats.last_round_cyles_burned, total_member_block_cycles
        ));
    }
    run();
    Ok(())
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
            run();
        })
    });
}

#[post_upgrade]
fn post_upgrade() {
    if get_miner().is_none() {
        trap("No miner found.");
    }
    run();
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
        set_member_rewards(caller, vec![]);

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
