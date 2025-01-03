use crate::{
    bob_transfer, get_last_reward_timestamp, get_latest_blocks, get_member_rewards,
    get_member_to_pending_cycles, push_member_rewards, reset_member_pending_cycles,
    set_last_reward_timestamp, set_member_rewards, GuardPrincipal, Reward, TaskGuard, TaskType,
};
use candid::Principal;
use std::cmp::max;

pub async fn check_rewards() -> Result<(), String> {
    let _guard_principal = TaskGuard::new(TaskType::CheckRewards)
        .map_err(|guard_error| format!("Concurrency error: {:?}", guard_error))?;
    let latest_blocks = get_latest_blocks().await?;
    let mut total_bob_rewards: u128 = 0;
    let last_reward_timestamp = get_last_reward_timestamp();
    let mut max_reward_timestamp = last_reward_timestamp;
    for block in latest_blocks {
        if block.to == ic_cdk::id() && block.timestamp > last_reward_timestamp {
            total_bob_rewards = total_bob_rewards.checked_add(block.rewards.into()).unwrap();
            max_reward_timestamp = max(max_reward_timestamp, block.timestamp);
        }
    }
    if total_bob_rewards > 0 {
        let new_rewards: Vec<(Principal, Reward)> = compute_rewards(total_bob_rewards);
        let members: Vec<Principal> = new_rewards
            .iter()
            .map(|(member, _)| member)
            .cloned()
            .collect();
        push_member_rewards(new_rewards);
        reset_member_pending_cycles(members);
        set_last_reward_timestamp(max_reward_timestamp);
    }
    Ok(())
}

fn compute_rewards(total_bob_brutto: u128) -> Vec<(Principal, Reward)> {
    let member_to_pending_cycles: Vec<(Principal, u128)> = get_member_to_pending_cycles();
    let total_pending_cycles: u128 = member_to_pending_cycles
        .iter()
        .map(|(_, pending_cycles)| pending_cycles)
        .sum();
    let num_members: u128 = member_to_pending_cycles.len() as u128;
    let total_bob_fee: u128 = num_members.checked_mul(1_000_000).unwrap();
    let total_bob_netto: u128 = total_bob_brutto.checked_sub(total_bob_fee).unwrap();
    let current_time: u64 = ic_cdk::api::time();
    member_to_pending_cycles
        .into_iter()
        .map(|(member, pending_cycles)| {
            let bob_reward: u128 = total_bob_netto
                .checked_mul(pending_cycles)
                .unwrap()
                .checked_div(total_pending_cycles)
                .unwrap();
            (
                member,
                Reward {
                    timestamp: current_time,
                    cycles_burnt: pending_cycles,
                    bob_reward,
                    bob_block_index: None,
                },
            )
        })
        .collect()
}

pub async fn pay_rewards(member: Principal) -> Result<(), String> {
    let _guard_principal = GuardPrincipal::new(member)
        .map_err(|guard_error| format!("Concurrency error: {:?}", guard_error))?;
    let mut res = Ok(());
    let mut rewards = get_member_rewards(member);
    for reward in &mut rewards {
        if reward.bob_block_index.is_none() {
            match bob_transfer(member, reward.bob_reward).await {
                Ok(block_idx) => {
                    reward.bob_block_index = Some(block_idx);
                }
                Err(err) => {
                    res = Err(err);
                    break;
                }
            }
        }
    }
    set_member_rewards(member, rewards);
    res
}
