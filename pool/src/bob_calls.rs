use crate::{MAINNET_BOB_CANISTER_ID, MAINNET_BOB_LEDGER_CANISTER_ID};
use bob_miner_v2::{MinerSettings, StatsV2};
use bob_minter_v2::{Block, Stats};
use candid::{Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{TransferArg, TransferError};

// BoB Ledger Canister

pub async fn bob_transfer(user_id: Principal, amount: u128) -> Result<u64, String> {
    ic_cdk::call::<_, (Result<Nat, TransferError>,)>(
        MAINNET_BOB_LEDGER_CANISTER_ID,
        "icrc1_transfer",
        (TransferArg {
            from_subaccount: None,
            to: Account {
                owner: user_id,
                subaccount: None,
            },
            fee: Some(1_000_000_u64.into()),
            created_at_time: None,
            memo: None,
            amount: amount.into(),
        },),
    )
    .await
    .map_err(|(code, msg)| {
        format!(
            "Error while calling BoB ledger canister ({:?}): {}",
            code, msg
        )
    })?
    .0
    .map(|block| block.0.try_into().unwrap())
    .map_err(|err| format!("Error from BoB ledger canister: {}", err))
}

// BoB Canister

pub async fn spawn_miner(block_index: u64) -> Result<Principal, String> {
    ic_cdk::call::<_, (Result<Principal, String>,)>(
        MAINNET_BOB_CANISTER_ID,
        "spawn_miner",
        (block_index,),
    )
    .await
    .map_err(|(code, msg)| format!("Error while calling BoB canister ({:?}): {}", code, msg))?
    .0
    .map_err(|err| format!("Error from BoB canister: {}", err))
}

pub async fn upgrade_miner(miner: Principal) -> Result<(), String> {
    ic_cdk::call::<_, (Result<(), String>,)>(MAINNET_BOB_CANISTER_ID, "upgrade_miner", (miner,))
        .await
        .map_err(|(code, msg)| format!("Error while calling BoB canister ({:?}): {}", code, msg))?
        .0
        .map_err(|err| format!("Error from BoB canister: {}", err))
}

pub async fn get_latest_blocks() -> Result<Vec<Block>, String> {
    ic_cdk::call::<_, (Vec<Block>,)>(MAINNET_BOB_CANISTER_ID, "get_latest_blocks", ((),))
        .await
        .map(|res| res.0)
        .map_err(|(code, msg)| format!("Error while calling BoB canister ({:?}): {}", code, msg))
}

pub async fn get_bob_statistics() -> Result<Stats, String> {
    ic_cdk::call::<_, (Stats,)>(MAINNET_BOB_CANISTER_ID, "get_statistics", ((),))
        .await
        .map(|res| res.0)
        .map_err(|(code, msg)| format!("Error while calling BoB canister ({:?}): {}", code, msg))
}

// BoB miner canister

pub async fn update_miner_settings(
    miner: Principal,
    max_cycles_per_round: Option<u128>,
    new_owner: Option<Principal>,
) -> Result<(), String> {
    let update_miner_settings_args = MinerSettings {
        max_cycles_per_round,
        new_owner,
    };
    ic_cdk::call::<_, ((),)>(
        miner,
        "update_miner_settings",
        (update_miner_settings_args,),
    )
    .await
    .map(|res| res.0)
    .map_err(|(code, msg)| format!("Error while calling miner ({:?}): {}", code, msg))
}

pub async fn get_miner_statistics(miner: Principal) -> Result<StatsV2, String> {
    ic_cdk::call::<_, (StatsV2,)>(miner, "get_statistics_v2", ((),))
        .await
        .map(|res| res.0)
        .map_err(|(code, msg)| format!("Error while calling miner canister ({:?}): {}", code, msg))
}
