use candid::{CandidType, Nat};
use serde::{Deserialize, Serialize};

#[derive(CandidType, Debug, Default, Serialize, Deserialize)]
pub struct MemberCycles {
    pub block: Nat,
    pub pending: Nat,
    pub remaining: Nat,
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct Reward {
    pub timestamp: u64,
    pub cycles_burnt: u128,
    pub bob_reward: u128,
    pub bob_block_index: Option<u64>,
}
