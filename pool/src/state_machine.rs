use crate::{
    check_rewards, commit_block_members, get_bob_statistics, get_miner_canister,
    get_miner_statistics, get_next_block_members, update_miner_settings, upgrade_miner,
};
use ic_cdk::api::canister_balance128;
use ic_cdk::api::management_canister::main::{deposit_cycles, CanisterIdRecord};
use ic_cdk::trap;
use std::future::Future;
use std::time::Duration;

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
                if max_attempts == 1 {
                    ic_cdk::print(format!(
                        "ERR(retry_and_log): Exceeded max attempts in {}: starting from scratch.",
                        phase
                    ));
                    run(retry_delay);
                } else {
                    retry_and_log(retry_delay, retry_delay, max_attempts - 1, phase, f, arg);
                }
            }
        });
    });
}

pub fn run(delay: Duration) {
    retry_and_log(delay, Duration::from_secs(0), 1, "stage_1", stage_1, ());
}

async fn stage_1(_: ()) -> Result<(), String> {
    let miner = get_miner_canister().unwrap();
    update_miner_settings(miner, Some(0), None).await?;
    check_rewards().await?;
    let stats = get_bob_statistics().await?;
    let time_since_last_block = stats.time_since_last_block;
    if time_since_last_block < 120 {
        retry_and_log(
            Duration::from_secs(0),
            Duration::from_secs(0),
            1,
            "stage_2",
            stage_2,
            (),
        );
    } else if time_since_last_block >= 490 {
        let block_count = stats.block_count;
        return Err(format!(
            "Time since last block {} too high: {}",
            block_count, time_since_last_block
        ));
    } else {
        retry_and_log(
            Duration::from_secs(490 - time_since_last_block),
            Duration::from_secs(0),
            1,
            "stage_1",
            stage_1,
            (),
        );
    }
    Ok(())
}

async fn stage_2(_: ()) -> Result<(), String> {
    let next_block_members = get_next_block_members();
    let total_member_block_cycles = next_block_members
        .iter()
        .map(|(_, block_cycles)| block_cycles)
        .sum();
    if total_member_block_cycles == 0 {
        run(Duration::from_secs(60));
        return Ok(());
    }
    let miner = get_miner_canister().unwrap();
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
    let miner = get_miner_canister().unwrap();
    deposit_cycles(CanisterIdRecord { canister_id: miner }, top_up_cycles)
        .await
        .map_err(|(code, msg)| {
            format!(
                "Error while depositing cycles to miner ({:?}): {}",
                code, msg
            )
        })?;
    commit_block_members(next_block_members);
    retry_and_log(
        Duration::from_secs(250),
        Duration::from_secs(10),
        3,
        "stage_3",
        stage_3,
        total_member_block_cycles,
    );
    Ok(())
}

async fn stage_3(total_member_block_cycles: u128) -> Result<(), String> {
    let miner = get_miner_canister().unwrap();
    let miner_stats = get_miner_statistics(miner).await?;
    if miner_stats.last_round_cyles_burned != total_member_block_cycles {
        return Err(format!(
            "Last cycles burned {} do not match the expectation {}.",
            miner_stats.last_round_cyles_burned, total_member_block_cycles
        ));
    }
    run(Duration::from_secs(0));
    Ok(())
}