use crate::{MemberCycles, Rewards};
use candid::{Nat, Principal};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager as MM, VirtualMemory};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::DefaultMemoryImpl;
use ic_stable_structures::{DefaultMemoryImpl as DefMem, StableBTreeMap, StableCell, Storable};
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

// NOTE: ensure that all memory ids are unique and
// do not change across upgrades!
const MINER_CANISTER_MEM_ID: MemoryId = MemoryId::new(0);
const MEMBER_TO_CYCLES_MEM_ID: MemoryId = MemoryId::new(1);
const REWARDS_MEM_ID: MemoryId = MemoryId::new(2);

type VM = VirtualMemory<DefMem>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MM<DefaultMemoryImpl>> = RefCell::new(
        MM::init(DefaultMemoryImpl::default())
    );

    static MINER_CANISTER: RefCell<StableCell<Option<Principal>, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableCell::init(mm.borrow().get(MINER_CANISTER_MEM_ID), None).unwrap())
    });

    static MEMBER_TO_CYCLES: RefCell<StableBTreeMap<Principal, Cbor<MemberCycles>, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(MEMBER_TO_CYCLES_MEM_ID)))
    });

    static REWARDS: RefCell<StableBTreeMap<u64, Cbor<Rewards>, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(REWARDS_MEM_ID)))
    });
}

pub fn get_miner_canister() -> Option<Principal> {
    MINER_CANISTER.with(|s| *s.borrow().get())
}

pub fn set_miner_canister(bob_miner_canister: Principal) {
    let _ = MINER_CANISTER.with(|s| s.borrow_mut().set(Some(bob_miner_canister)).unwrap());
}

pub fn add_member_total_cycles(member: Principal, new_cycles: u128) {
    MEMBER_TO_CYCLES.with(|s| {
        let mut member_cycles = s.borrow().get(&member).unwrap_or_default();
        member_cycles.0.remaining += new_cycles;
        s.borrow_mut().insert(member, member_cycles)
    });
}

pub fn set_member_block_cycles(member: Principal, block_cycles: Nat) {
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
                if mc.0.block.clone() + 5_000_000_000_u64 <= mc.0.remaining {
                    let block_cycles: u128 = mc.0.block.0.try_into().unwrap();
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
            let mut mc = s.borrow().get(&member).unwrap().0;
            mc.remaining -= block_cycles + fee;
            mc.pending += block_cycles;
            s.borrow_mut().insert(member, Cbor(mc));
        }
    });
}

pub fn add_rewards(total_bob: u128) -> u64 {
    let num_beneficiaries = MEMBER_TO_CYCLES.with(|s| {
        s.borrow()
            .iter()
            .filter(|(_, mc)| mc.0.pending != 0_u64)
            .count()
    });
    let total_bob_fee = num_beneficiaries.checked_mul(1_000_000).unwrap() as u128;
    let distribute_bob = total_bob.checked_sub(total_bob_fee).unwrap();
    let total_cycles = MEMBER_TO_CYCLES.with(|s| {
        s.borrow()
            .iter()
            .map(|(_, mc)| {
                let pending: u128 = mc.0.pending.0.try_into().unwrap();
                pending
            })
            .sum::<u128>()
    });
    let participants: Vec<(Principal, u128)> = MEMBER_TO_CYCLES.with(|s| {
        s.borrow()
            .iter()
            .filter_map(|(member, mc)| {
                if mc.0.pending != 0_u64 {
                    let pending: u128 = mc.0.pending.0.try_into().unwrap();
                    Some((member, distribute_bob * pending / total_cycles))
                } else {
                    None
                }
            })
            .collect()
    });
    let rewards = Cbor(Rewards {
        total_amount: total_bob,
        pending: participants.iter().map(|(_, amount)| amount).sum::<u128>() + total_bob_fee,
        participants,
        transfer_idx: vec![],
    });
    REWARDS.with(|s| {
        let n = s.borrow().len();
        s.borrow_mut().insert(n, rewards);
        n
    })
}

pub fn total_pending_rewards() -> u128 {
    REWARDS.with(|s| s.borrow().iter().map(|(_, r)| r.0.pending).sum())
}

pub fn get_rewards(idx: u64) -> Rewards {
    REWARDS.with(|s| s.borrow().get(&idx).unwrap().0)
}

pub fn set_rewards(idx: u64, rewards: Rewards) {
    REWARDS.with(|s| s.borrow_mut().insert(idx, Cbor(rewards)));
}
