pub use crate::bob_calls::{
    bob_transfer, get_bob_statistics, get_latest_blocks, get_miner_statistics, spawn_miner,
    update_miner_settings, upgrade_miner,
};
pub use crate::guard::{GuardPrincipal, TaskGuard, TaskType};
pub use crate::memory::{
    add_member_remaining_cycles, commit_block_members, get_and_set_block_count,
    get_last_reward_timestamp, get_member_cycles, get_member_rewards, get_member_to_pending_cycles,
    get_miner_canister, get_next_block_members, init_member_rewards, push_member_rewards,
    reset_member_pending_cycles, set_last_reward_timestamp, set_member_block_cycles,
    set_member_rewards, set_miner_canister,
};
pub use crate::rewards::{check_rewards, pay_rewards};
pub use crate::state_machine::run;
pub use crate::system_calls::{fetch_block, notify_top_up, transfer};
pub use crate::types::{MemberCycles, PoolRunningState, PoolState, Reward};

mod bob_calls;
mod guard;
mod memory;
mod rewards;
mod state_machine;
mod system_calls;
mod types;

use candid::Principal;
use std::cell::RefCell;
use std::collections::BTreeSet;

pub const BOB_POOL_BLOCK_FEE: u128 = 5_000_000_000;

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

pub fn get_pool_state() -> PoolState {
    PoolState {
        miner: get_miner_canister(),
        running_state: get_running_state(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub running_state: PoolRunningState,
    pub principal_guards: BTreeSet<Principal>,
    pub active_tasks: BTreeSet<TaskType>,
}

pub fn get_running_state() -> PoolRunningState {
    __STATE.with(|s| s.borrow().running_state)
}

pub fn start() {
    match get_running_state() {
        PoolRunningState::Running => (),
        PoolRunningState::Stopping => {
            running();
        }
        PoolRunningState::Stopped => {
            running();
            run(std::time::Duration::from_secs(0));
        }
    };
}

pub fn stop() {
    if let PoolRunningState::Running = get_running_state() {
        stopping();
    }
}

fn running() {
    mutate_state(|s| {
        s.running_state = PoolRunningState::Running;
    });
}

pub(crate) fn stopping() {
    mutate_state(|s| {
        s.running_state = PoolRunningState::Stopping;
    });
}

pub(crate) fn stopped() {
    mutate_state(|s| {
        s.running_state = PoolRunningState::Stopped;
    });
}

pub fn mutate_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut State) -> R,
{
    __STATE.with(|s| f(&mut s.borrow_mut()))
}
