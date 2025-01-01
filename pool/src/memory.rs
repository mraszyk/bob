use crate::{MemberCycles, Reward};
use candid::{Nat, Principal};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager as MM, VirtualMemory};
use ic_stable_structures::DefaultMemoryImpl;
use ic_stable_structures::{DefaultMemoryImpl as DefMem, StableBTreeMap, StableCell};
use orbit_essentials_macros::storable;
use std::cell::RefCell;

#[derive(Clone, Copy, Default)]
#[storable]
struct State {
    pub miner: Option<Principal>,
    pub last_reward_timestamp: u64,
}

#[derive(Clone, Default)]
#[storable]
struct Rewards(pub Vec<Reward>);

// NOTE: ensure that all memory ids are unique and
// do not change across upgrades!
const POOL_STATE_MEM_ID: MemoryId = MemoryId::new(0);
const MEMBER_TO_CYCLES_MEM_ID: MemoryId = MemoryId::new(1);
const MEMBER_TO_REWARDS_MEM_ID: MemoryId = MemoryId::new(2);

type VM = VirtualMemory<DefMem>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MM<DefaultMemoryImpl>> = RefCell::new(
        MM::init(DefaultMemoryImpl::default())
    );

    static POOL_STATE: RefCell<StableCell<State, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableCell::init(mm.borrow().get(POOL_STATE_MEM_ID), State::default()).unwrap())
    });

    static MEMBER_TO_CYCLES: RefCell<StableBTreeMap<Principal, MemberCycles, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(MEMBER_TO_CYCLES_MEM_ID)))
    });

    static MEMBER_TO_REWARDS: RefCell<StableBTreeMap<Principal, Rewards, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(MEMBER_TO_REWARDS_MEM_ID)))
    });
}

pub fn get_miner_canister() -> Option<Principal> {
    POOL_STATE.with(|s| s.borrow().get().miner)
}

pub fn set_miner_canister(miner: Principal) {
    POOL_STATE.with(|s| {
        let mut state = *s.borrow().get();
        state.miner = Some(miner);
        s.borrow_mut().set(state).unwrap();
    });
}

pub fn get_last_reward_timestamp() -> u64 {
    POOL_STATE.with(|s| s.borrow().get().last_reward_timestamp)
}

pub fn set_last_reward_timestamp(last_reward_timestamp: u64) {
    POOL_STATE.with(|s| {
        let mut state = *s.borrow().get();
        state.last_reward_timestamp = last_reward_timestamp;
        s.borrow_mut().set(state).unwrap();
    });
}

pub fn add_member_remaining_cycles(member: Principal, new_cycles: u128) {
    MEMBER_TO_CYCLES.with(|s| {
        let mut member_cycles = s.borrow().get(&member).unwrap_or_default();
        member_cycles.remaining += new_cycles;
        s.borrow_mut().insert(member, member_cycles)
    });
}

pub fn set_member_block_cycles(member: Principal, block_cycles: Nat) {
    MEMBER_TO_CYCLES.with(|s| {
        let mut member_cycles = s.borrow().get(&member).unwrap_or_default();
        member_cycles.block = block_cycles;
        s.borrow_mut().insert(member, member_cycles)
    });
}

pub fn get_member_cycles(member: Principal) -> Option<MemberCycles> {
    MEMBER_TO_CYCLES.with(|s| s.borrow().get(&member))
}

pub fn get_next_block_participants() -> Vec<(Principal, u128)> {
    MEMBER_TO_CYCLES.with(|s| {
        s.borrow()
            .iter()
            .filter_map(|(member, mc)| {
                if mc.block.clone() + 5_000_000_000_u64 <= mc.remaining {
                    let block_cycles: u128 = mc.block.0.try_into().unwrap();
                    Some((member, block_cycles))
                } else {
                    None
                }
            })
            .collect()
    })
}

pub fn commit_block_participants(participants: Vec<(Principal, u128)>) {
    let fee = 5_000_000_000_u128 / (participants.len() as u128);
    MEMBER_TO_CYCLES.with(|s| {
        for (member, block_cycles) in participants {
            let mut mc = s.borrow().get(&member).unwrap();
            mc.remaining -= block_cycles + fee;
            mc.pending += block_cycles;
            s.borrow_mut().insert(member, mc);
        }
    });
}

pub fn add_rewards(total_bob_brutto: u128) {
    let participants: Vec<_> = MEMBER_TO_CYCLES.with(|s| {
        s.borrow()
            .iter()
            .filter_map(|(member, mc)| {
                if mc.pending != 0_u64 {
                    Some(member)
                } else {
                    None
                }
            })
            .collect()
    });
    let num_participants = participants.len() as u128;
    let total_bob_fee = num_participants.checked_mul(1_000_000).unwrap();
    let total_bob_netto = total_bob_brutto.checked_sub(total_bob_fee).unwrap();
    let total_pending_cycles = MEMBER_TO_CYCLES.with(|s| {
        s.borrow()
            .iter()
            .map(|(_, mc)| {
                let pending_cycles: u128 = mc.pending.0.try_into().unwrap();
                pending_cycles
            })
            .sum::<u128>()
    });
    MEMBER_TO_CYCLES.with(|s| {
        for (member, mc) in s.borrow().iter() {
            if mc.pending != 0_u64 {
                let pending_cycles: u128 = mc.pending.0.try_into().unwrap();
                let bob_reward = total_bob_netto
                    .checked_mul(pending_cycles)
                    .unwrap()
                    .checked_div(total_pending_cycles)
                    .unwrap();
                MEMBER_TO_REWARDS.with(|s| {
                    let mut rewards = s.borrow().get(&member).unwrap();
                    rewards.0.push(Reward {
                        timestamp: ic_cdk::api::time(),
                        cycles_burnt: pending_cycles,
                        bob_reward,
                        bob_block_index: None,
                    });
                    s.borrow_mut().insert(member, rewards);
                });
            }
        }
    });
    MEMBER_TO_CYCLES.with(|s| {
        for member in participants.iter() {
            let mut mc = s.borrow().get(member).unwrap();
            mc.pending = 0_u64.into();
            s.borrow_mut().insert(*member, mc);
        }
    });
}

pub fn get_member_rewards(member: Principal) -> Vec<Reward> {
    MEMBER_TO_REWARDS.with(|s| s.borrow().get(&member).unwrap_or_default().0)
}

pub fn set_member_rewards(member: Principal, rewards: Vec<Reward>) {
    MEMBER_TO_REWARDS.with(|s| s.borrow_mut().insert(member, Rewards(rewards)));
}
