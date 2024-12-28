use candid::Principal;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager as MM, VirtualMemory};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::DefaultMemoryImpl;
use ic_stable_structures::{DefaultMemoryImpl as DefMem, StableBTreeMap, Storable};
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
const MINER_TO_CYCLES_MEM_ID: MemoryId = MemoryId::new(0);

type VM = VirtualMemory<DefMem>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MM<DefaultMemoryImpl>> = RefCell::new(
        MM::init(DefaultMemoryImpl::default())
    );

    static MINER_TO_CYCLES: RefCell<StableBTreeMap<Principal, u128, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(MINER_TO_CYCLES_MEM_ID)))
        });
}

pub fn insert_new_miner(miner: Principal, cycles: u128) {
    MINER_TO_CYCLES.with(|s| s.borrow_mut().insert(miner, cycles));
}

pub fn get_miner_cycles(miner: Principal) -> Option<u128> {
    MINER_TO_CYCLES.with(|s| s.borrow().get(&miner))
}
