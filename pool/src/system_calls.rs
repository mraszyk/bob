use crate::{
    MAINNET_CYCLE_MINTER_CANISTER_ID, MAINNET_LEDGER_CANISTER_ID, MAINNET_LEDGER_INDEX_CANISTER_ID,
};
use candid::{CandidType, Nat, Principal};
use cycles_minting_canister::NotifyError;
use ic_ledger_core::block::BlockType;
use ic_ledger_types::TransferResult;
use ic_types::Cycles;
use icp_ledger::{AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs};

// ICP Ledger

pub async fn transfer(
    account_principal: Principal,
    subaccount_principal: Option<Principal>,
    memo: u64,
    amount: u64,
) -> Result<u64, String> {
    let to = AccountIdentifier::new(
        ic_types::PrincipalId(account_principal),
        subaccount_principal.map(|p| Subaccount::from(&ic_types::PrincipalId(p))),
    );
    let transfer_args = TransferArgs {
        memo: Memo(memo),
        amount: Tokens::from_e8s(amount),
        from_subaccount: None,
        fee: Tokens::from_e8s(10_000),
        to: to.to_address(),
        created_at_time: None,
    };
    ic_cdk::call::<_, (TransferResult,)>(MAINNET_LEDGER_CANISTER_ID, "transfer", (transfer_args,))
        .await
        .map_err(|(code, msg)| {
            format!(
                "Error while calling ICP ledger canister ({:?}): {}",
                code, msg
            )
        })?
        .0
        .map_err(|err| format!("Error from ICP ledger canister: {}", err))
}

// ICP Index

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

// Cycles Minting Canister

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
