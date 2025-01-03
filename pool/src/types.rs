use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

#[derive(CandidType, Debug, Default, Serialize, Deserialize)]
pub struct MemberCycles {
    pub block: u128,
    pub pending: u128,
    pub remaining: u128,
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct Reward {
    pub timestamp: u64,
    pub cycles_burnt: u128,
    pub bob_reward: u128,
    pub bob_block_index: Option<u64>,
}

#[derive(CandidType, Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum PoolRunningState {
    Running,
    Stopping,
    #[default]
    Stopped,
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct PoolState {
    pub miner: Option<Principal>,
    pub running_state: PoolRunningState,
    pub num_active_members: u64,
    pub total_active_member_block_cycles: u128,
    pub total_cycles_burnt: u128,
    pub total_bob_rewards: u128,
}
