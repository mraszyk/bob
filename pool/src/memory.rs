use crate::{MemberCycles, Reward, BOB_POOL_BLOCK_FEE};
use candid::Principal;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager as MM, VirtualMemory};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::DefaultMemoryImpl;
use ic_stable_structures::{DefaultMemoryImpl as DefMem, StableBTreeMap, StableCell, Storable};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::RefCell;

#[derive(Default, Ord, PartialOrd, Clone, Eq, PartialEq)]
struct Cbor<T>(pub T)
where
    T: serde::Serialize + serde::de::DeserializeOwned;

impl<T> Storable for Cbor<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn to_bytes(&self) -> Cow<[u8]> {
        let mut buf = vec![];
        ciborium::ser::into_writer(&self.0, &mut buf).unwrap();
        Cow::Owned(buf)
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Self(ciborium::de::from_reader(bytes.as_ref()).unwrap())
    }

    const BOUND: Bound = Bound::Unbounded;
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
struct State {
    pub miner: Option<Principal>,
    pub last_reward_timestamp: u64,
}

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

    static POOL_STATE: RefCell<StableCell<Cbor<State>, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableCell::init(mm.borrow().get(POOL_STATE_MEM_ID), Cbor(State::default())).unwrap())
    });

    static MEMBER_TO_CYCLES: RefCell<StableBTreeMap<Principal, Cbor<MemberCycles>, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(MEMBER_TO_CYCLES_MEM_ID)))
    });

    static MEMBER_TO_REWARDS: RefCell<StableBTreeMap<Principal, Cbor<Vec<Reward>>, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(MEMBER_TO_REWARDS_MEM_ID)))
    });
}

pub fn get_miner_canister() -> Option<Principal> {
    POOL_STATE.with(|s| s.borrow().get().0.miner)
}

pub fn set_miner_canister(miner: Principal) {
    POOL_STATE.with(|s| {
        let mut state = s.borrow().get().clone();
        state.0.miner = Some(miner);
        s.borrow_mut().set(state).unwrap();
    });
}

pub fn get_last_reward_timestamp() -> u64 {
    POOL_STATE.with(|s| s.borrow().get().0.last_reward_timestamp)
}

pub fn set_last_reward_timestamp(last_reward_timestamp: u64) {
    POOL_STATE.with(|s| {
        let mut state = s.borrow().get().clone();
        state.0.last_reward_timestamp = last_reward_timestamp;
        s.borrow_mut().set(state).unwrap();
    });
}

pub fn add_member_remaining_cycles(member: Principal, new_cycles: u128) {
    MEMBER_TO_CYCLES.with(|s| {
        let mut member_cycles = s.borrow().get(&member).unwrap_or_default();
        member_cycles.0.remaining += new_cycles;
        s.borrow_mut().insert(member, member_cycles)
    });
}

pub fn set_member_block_cycles(member: Principal, block_cycles: u128) {
    MEMBER_TO_CYCLES.with(|s| {
        let mut member_cycles = s.borrow().get(&member).unwrap_or_default();
        member_cycles.0.block = block_cycles;
        s.borrow_mut().insert(member, member_cycles)
    });
}

pub fn get_member_cycles(member: Principal) -> Option<MemberCycles> {
    MEMBER_TO_CYCLES.with(|s| s.borrow().get(&member).map(|mc| mc.0))
}

pub fn get_next_block_participants() -> Vec<(Principal, u128)> {
    MEMBER_TO_CYCLES.with(|s| {
        s.borrow()
            .iter()
            .filter_map(|(member, mc)| {
                if mc.0.block + BOB_POOL_BLOCK_FEE <= mc.0.remaining {
                    Some((member, mc.0.block))
                } else {
                    None
                }
            })
            .collect()
    })
}

pub fn commit_block_participants(participants: Vec<(Principal, u128)>) {
    let per_member_fee = BOB_POOL_BLOCK_FEE / (participants.len() as u128);
    MEMBER_TO_CYCLES.with(|s| {
        for (member, block_cycles) in participants {
            let mut mc = s.borrow().get(&member).unwrap();
            mc.0.remaining =
                mc.0.remaining
                    .checked_sub(block_cycles + per_member_fee)
                    .unwrap();
            mc.0.pending += block_cycles;
            s.borrow_mut().insert(member, mc);
        }
    });
}

pub fn get_member_to_pending_cycles() -> Vec<(Principal, u128)> {
    MEMBER_TO_CYCLES.with(|s| {
        s.borrow()
            .iter()
            .filter_map(|(member, mc)| {
                if mc.0.pending != 0 {
                    Some((member, mc.0.pending))
                } else {
                    None
                }
            })
            .collect()
    })
}

pub fn reset_member_pending_cycles(members: Vec<Principal>) {
    MEMBER_TO_CYCLES.with(|s| {
        for member in members {
            let mut mc = s.borrow().get(&member).unwrap();
            mc.0.pending = 0;
            s.borrow_mut().insert(member, mc);
        }
    });
}

pub fn push_member_rewards(rewards: Vec<(Principal, Reward)>) {
    MEMBER_TO_REWARDS.with(|s| {
        for (member, reward) in rewards {
            let mut rewards = s.borrow().get(&member).unwrap();
            rewards.0.push(reward);
            s.borrow_mut().insert(member, rewards);
        }
    });
}

pub fn get_member_rewards(member: Principal) -> Vec<Reward> {
    MEMBER_TO_REWARDS.with(|s| s.borrow().get(&member).map(|r| r.0).unwrap_or_default())
}

pub fn set_member_rewards(member: Principal, rewards: Vec<Reward>) {
    MEMBER_TO_REWARDS.with(|s| s.borrow_mut().insert(member, Cbor(rewards)));
}
