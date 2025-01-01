use candid::{CandidType, Nat};
use orbit_essentials_macros::storable;

#[derive(CandidType, Debug, Default)]
#[storable]
pub struct MemberCycles {
    pub block: Nat,
    pub pending: Nat,
    pub remaining: Nat,
}

#[derive(CandidType, Clone, Debug)]
#[storable]
pub struct Reward {
    pub timestamp: u64,
    pub cycles_burnt: u128,
    pub bob_reward: u128,
    pub bob_block_index: Option<u64>,
}
