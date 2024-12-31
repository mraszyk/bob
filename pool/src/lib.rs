use crate::guard::TaskType;
use candid::{CandidType, Nat, Principal};
use cycles_minting_canister::NotifyError;
use ic_ledger_core::block::BlockType;
use ic_types::Cycles;
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

pub mod guard;
pub mod memory;
mod types;

pub use crate::types::*;

pub async fn fetch_block(block_height: u64) -> Result<icp_ledger::Block, String> {
    let args = icrc_ledger_types::icrc3::blocks::GetBlocksRequest {
        start: block_height.into(),
        length: Nat::from(1_u8),
    };

    let res = ic_cdk::api::call::call::<_, (ic_icp_index::GetBlocksResponse,)>(
        MAINNET_LEDGER_INDEX_CANISTER_ID,
        "get_blocks",
        (args,),
    )
    .await;
    match res {
        Ok(res) => {
            if let Some(block_raw) = res.0.blocks.first() {
                Ok(icp_ledger::Block::decode(block_raw.clone()).unwrap())
            } else {
                Err(format!(
                    "Block {} not available in ICP index canister",
                    block_height
                ))
            }
        }
        Err((code, msg)) => Err(format!(
            "Error while calling ICP index canister ({:?}): {}",
            code, msg
        )),
    }
}

#[derive(CandidType)]
struct NotifyTopUp {
    block_index: u64,
    canister_id: Principal,
}

pub async fn notify_top_up(block_height: u64) -> Result<Cycles, String> {
    let canister_id = ic_cdk::id();
    let args = NotifyTopUp {
        block_index: block_height,
        canister_id,
    };

    let res = ic_cdk::api::call::call::<_, (Result<Cycles, NotifyError>,)>(
        MAINNET_CYCLE_MINTER_CANISTER_ID,
        "notify_top_up",
        (args,),
    )
    .await;
    match res {
        Ok(res) => match res.0 {
            Ok(cycles) => Ok(cycles),
            Err(e) => Err(format!("Error from cycles minting canister: {e}")),
        },
        Err((code, msg)) => Err(format!(
            "Error while calling cycles minting canister ({:?}): {}",
            code, msg
        )),
    }
}

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
