use candid::{CandidType, Nat, Principal};
use serde::{Deserialize, Serialize};

#[derive(CandidType, Debug, Default, Serialize, Deserialize)]
pub struct MemberCycles {
    pub block: Nat,
    pub pending: Nat,
    pub remaining: Nat,
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct Rewards {
    pub total_amount: u128,
    pub pending: u128,
    pub participants: Vec<(Principal, u128)>,
    pub transfer_idx: Vec<(Principal, u64)>,
}
