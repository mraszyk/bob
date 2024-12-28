use candid::Principal;
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
const BOB_MINER_CANISTER_MEM_ID: MemoryId = MemoryId::new(0);
const MEMBER_TO_CYCLES_MEM_ID: MemoryId = MemoryId::new(1);

type VM = VirtualMemory<DefMem>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MM<DefaultMemoryImpl>> = RefCell::new(
        MM::init(DefaultMemoryImpl::default())
    );

    static BOB_MINER_CANISTER: RefCell<StableCell<Option<Principal>, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableCell::init(mm.borrow().get(BOB_MINER_CANISTER_MEM_ID), None).unwrap())
        });

    static MEMBER_TO_CYCLES: RefCell<StableBTreeMap<Principal, u128, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(MEMBER_TO_CYCLES_MEM_ID)))
        });
}

pub fn get_miner_canister() -> Option<Principal> {
    BOB_MINER_CANISTER.with(|s| *s.borrow().get())
}

pub fn set_miner_canister(bob_miner_canister: Principal) {
    let _ = BOB_MINER_CANISTER.with(|s| s.borrow_mut().set(Some(bob_miner_canister)).unwrap());
}

pub fn add_member_cycles(member: Principal, new_cycles: u128) {
    MEMBER_TO_CYCLES.with(|s| {
        let current_cycles = s.borrow().get(&member).unwrap_or(0);
        s.borrow_mut().insert(member, current_cycles + new_cycles)
    });
}

pub fn get_member_cycles(member: Principal) -> Option<u128> {
    MEMBER_TO_CYCLES.with(|s| s.borrow().get(&member))
}
