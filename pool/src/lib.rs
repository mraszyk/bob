use candid::{CandidType, Decode, Encode, Nat, Principal};
use cycles_minting_canister::NotifyError;
use ic_ledger_core::block::BlockType;
use ic_types::Cycles;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

pub const SEC_NANOS: u64 = 1_000_000_000;
pub const DAY_NANOS: u64 = 24 * 60 * 60 * SEC_NANOS;

pub const MAINNET_BOB_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x02, 0x40, 0x00, 0x55, 0x01, 0x01]);

pub const MAINNET_LEDGER_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x01, 0x01]);

pub const MAINNET_LEDGER_INDEX_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0B, 0x01, 0x01]);

pub const MAINNET_CYCLE_MINTER_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x01, 0x01]);

pub mod guard;
pub mod memory;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskType {
    ProcessLogic,
    MineBob,
}

#[derive(CandidType)]
struct NotifyTopUp {
    block_index: u64,
    canister_id: Principal,
}

pub async fn fetch_block(block_height: u64) -> Result<icp_ledger::Block, String> {
    let args = Encode!(&icrc_ledger_types::icrc3::blocks::GetBlocksRequest {
        start: block_height.into(),
        length: Nat::from(1_u8),
    })
    .unwrap();

    let result: Result<Vec<u8>, (i32, String)> = ic_cdk::api::call::call_raw(
        Principal::from_text("qhbym-qaaaa-aaaaa-aaafq-cai").unwrap(),
        "get_blocks",
        args,
        0,
    )
    .await
    .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok(res) => {
            let blocks = Decode!(&res, ic_icp_index::GetBlocksResponse).unwrap();
            icp_ledger::Block::decode(blocks.blocks.first().expect("no block").clone())
        }
        Err((code, msg)) => Err(format!(
            "Error while calling minter canister ({}): {:?}",
            code, msg
        )),
    }
}

pub async fn notify_top_up(block_height: u64) -> Result<Cycles, String> {
    let canister_id = ic_cdk::id();
    let args = Encode!(&NotifyTopUp {
        block_index: block_height,
        canister_id,
    })
    .unwrap();

    let res_gov: Result<Vec<u8>, (i32, String)> =
        ic_cdk::api::call::call_raw(MAINNET_CYCLE_MINTER_CANISTER_ID, "notify_top_up", args, 0)
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match res_gov {
        Ok(res) => {
            let decode = Decode!(&res, Result<Cycles, NotifyError>).unwrap();
            match decode {
                Ok(cycles) => Ok(cycles),
                Err(e) => Err(format!("{e}")),
            }
        }
        Err((code, msg)) => Err(format!(
            "Error while calling minter canister ({}): {:?}",
            code, msg
        )),
    }
}

thread_local! {
    static __STATE: RefCell<Option<State>> = RefCell::default();
}

#[derive(Clone, Debug)]
pub struct State {
    pub bob_canister_id: Principal,
    pub bob_ledger_id: Principal,
    pub bob_miner_id: Principal,

    pub principal_to_cycles: BTreeMap<Principal, u64>,
    pub principal_guards: BTreeSet<Principal>,
    pub active_tasks: BTreeSet<TaskType>,
}

impl State {
    pub fn new(bob_miner_id: Principal) -> Self {
        Self {
            bob_canister_id: Principal::from_text("6lnhz-oaaaa-aaaas-aabkq-cai").unwrap(),
            bob_ledger_id: Principal::from_text("7pail-xaaaa-aaaas-aabmq-cai").unwrap(),
            bob_miner_id,
            principal_to_cycles: BTreeMap::default(),
            principal_guards: BTreeSet::default(),
            active_tasks: BTreeSet::default(),
        }
    }
}

pub fn is_state_initialized() -> bool {
    __STATE.with(|s| s.borrow_mut().is_some())
}

pub fn mutate_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut State) -> R,
{
    __STATE.with(|s| f(s.borrow_mut().as_mut().expect("State not initialized!")))
}

pub fn read_state<F, R>(f: F) -> R
where
    F: FnOnce(&State) -> R,
{
    __STATE.with(|s| f(s.borrow().as_ref().expect("State not initialized!")))
}

pub fn replace_state(state: State) {
    __STATE.with(|s| {
        *s.borrow_mut() = Some(state);
    });
}