pub use crate::bob_calls::{
    bob_transfer, get_bob_balance, get_bob_statistics, get_latest_blocks, get_miner_statistics,
    spawn_miner, update_miner_settings, upgrade_miner,
};
pub use crate::guard::{GuardPrincipal, TaskGuard, TaskType};
pub use crate::memory::{
    add_member_remaining_cycles, add_rewards, commit_block_participants, get_last_reward_timestamp,
    get_member_cycles, get_member_rewards, get_miner_canister, get_next_block_participants,
    set_last_reward_timestamp, set_member_block_cycles, set_member_rewards, set_miner_canister,
};
pub use crate::system_calls::{fetch_block, notify_top_up, transfer};
pub use crate::types::{MemberCycles, Reward};

mod bob_calls;
mod guard;
mod memory;
mod system_calls;
mod types;

use candid::Principal;
use std::cell::RefCell;
use std::collections::BTreeSet;

pub const MAINNET_BOB_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x02, 0x40, 0x00, 0x55, 0x01, 0x01]);

pub const MAINNET_BOB_LEDGER_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x02, 0x40, 0x00, 0x59, 0x01, 0x01]);

pub const MAINNET_LEDGER_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x01, 0x01]);

pub const MAINNET_LEDGER_INDEX_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0B, 0x01, 0x01]);

pub const MAINNET_CYCLE_MINTER_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x01, 0x01]);

thread_local! {
    static __STATE: RefCell<State> = RefCell::default();
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub principal_guards: BTreeSet<Principal>,
    pub active_tasks: BTreeSet<TaskType>,
}

pub fn mutate_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut State) -> R,
{
    __STATE.with(|s| f(&mut s.borrow_mut()))
}
