use crate::{bob_transfer, get_member_rewards, set_member_rewards, GuardPrincipal};
use candid::Principal;

pub async fn pay_rewards(member: Principal) -> Result<(), String> {
    let _guard_principal = GuardPrincipal::new(member)
        .map_err(|guard_error| format!("Concurrency error: {:?}", guard_error))?;
    let mut rewards = get_member_rewards(member);
    let mut res = Ok(());
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
