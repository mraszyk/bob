use crate::setup::XDR_PERMYRIAD_PER_ICP;
use crate::{
    BOB_CANISTER_ID, BOB_LEDGER_CANISTER_ID, BOB_POOL_CANISTER_ID, NNS_CYCLES_MINTING_CANISTER_ID,
    NNS_ICP_INDEX_CANISTER_ID, NNS_ICP_LEDGER_CANISTER_ID,
};
use bob_miner_v2::MinerSettings;
use bob_minter_v2::{Block, Stats};
use bob_pool::{MemberCycles, PoolRunningState, PoolState, Reward};
use candid::{Encode, Nat, Principal};
use ic_ledger_core::block::BlockType;
use ic_ledger_types::{
    AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferResult, DEFAULT_SUBACCOUNT,
};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::management_canister::CanisterLogRecord;
use pocket_ic::{query_candid_as, update_candid_as, CallError, PocketIc, WasmResult};

pub(crate) fn get_icp_block(pic: &PocketIc, block_index: u64) -> Option<icp_ledger::Block> {
    let get_blocks_args = icrc_ledger_types::icrc3::blocks::GetBlocksRequest {
        start: block_index.into(),
        length: Nat::from(1_u8),
    };
    let blocks_raw = update_candid_as::<_, (ic_icp_index::GetBlocksResponse,)>(
        pic,
        NNS_ICP_INDEX_CANISTER_ID,
        Principal::anonymous(),
        "get_blocks",
        (get_blocks_args,),
    )
    .unwrap()
    .0;
    blocks_raw
        .blocks
        .first()
        .map(|block_raw| icp_ledger::Block::decode(block_raw.clone()).unwrap())
}

pub(crate) fn transfer_topup_bob(pic: &PocketIc, user_id: Principal, amount: u64) -> u64 {
    let sub = Subaccount::from(BOB_CANISTER_ID);
    let to = AccountIdentifier::new(&NNS_CYCLES_MINTING_CANISTER_ID, &sub);
    transfer(pic, user_id, to, amount)
}

pub(crate) fn transfer_topup_pool(pic: &PocketIc, user_id: Principal, amount: u64) -> u64 {
    let sub = Subaccount::from(BOB_POOL_CANISTER_ID);
    let to = AccountIdentifier::new(&NNS_CYCLES_MINTING_CANISTER_ID, &sub);
    transfer(pic, user_id, to, amount)
}

pub(crate) fn transfer_to_principal(
    pic: &PocketIc,
    user_id: Principal,
    beneficiary: Principal,
    amount: u64,
) -> u64 {
    transfer(
        pic,
        user_id,
        AccountIdentifier::new(&beneficiary, &DEFAULT_SUBACCOUNT),
        amount,
    )
}

fn transfer(pic: &PocketIc, user_id: Principal, to: AccountIdentifier, amount: u64) -> u64 {
    let transfer_args = TransferArgs {
        memo: Memo(1347768404),
        amount: Tokens::from_e8s(amount),
        from_subaccount: None,
        fee: Tokens::from_e8s(10_000),
        to,
        created_at_time: None,
    };
    let block_index = update_candid_as::<_, (TransferResult,)>(
        pic,
        NNS_ICP_LEDGER_CANISTER_ID,
        user_id,
        "transfer",
        (transfer_args,),
    )
    .unwrap()
    .0
    .unwrap();

    // wait for the ICP index to sync
    while get_icp_block(pic, block_index).is_none() {
        pic.advance_time(std::time::Duration::from_secs(1));
        pic.tick();
    }

    block_index
}

pub(crate) fn spawn_miner(pic: &PocketIc, user_id: Principal, amount: u64) -> Principal {
    let block_index = transfer_topup_bob(pic, user_id, amount);

    update_candid_as::<_, (Result<Principal, String>,)>(
        pic,
        BOB_CANISTER_ID,
        user_id,
        "spawn_miner",
        (block_index,),
    )
    .unwrap()
    .0
    .unwrap()
}

pub(crate) fn upgrade_miner(pic: &PocketIc, user_id: Principal, miner_id: Principal) {
    update_candid_as::<_, (Result<(), String>,)>(
        pic,
        BOB_CANISTER_ID,
        user_id,
        "upgrade_miner",
        (miner_id,),
    )
    .unwrap()
    .0
    .unwrap()
}

pub(crate) fn join_native_pool(
    pic: &PocketIc,
    user_id: Principal,
    amount: u64,
) -> Result<(), String> {
    let block_index = transfer_topup_bob(pic, user_id, amount);
    join_pool_(pic, user_id, BOB_CANISTER_ID, block_index)
}

pub(crate) fn join_pool(pic: &PocketIc, user_id: Principal, amount: u64) -> Result<(), String> {
    let block_index = transfer_topup_pool(pic, user_id, amount);
    join_pool_(pic, user_id, BOB_POOL_CANISTER_ID, block_index)
}

fn join_pool_(
    pic: &PocketIc,
    user_id: Principal,
    canister_id: Principal,
    block_index: u64,
) -> Result<(), String> {
    update_candid_as::<_, (Result<(), String>,)>(
        pic,
        canister_id,
        user_id,
        "join_pool",
        (block_index,),
    )
    .unwrap()
    .0
}

pub(crate) fn get_stats(pic: &PocketIc) -> Stats {
    update_candid_as::<_, (Stats,)>(
        pic,
        BOB_CANISTER_ID,
        Principal::anonymous(),
        "get_statistics",
        ((),),
    )
    .unwrap()
    .0
}

pub(crate) fn get_latest_blocks(pic: &PocketIc) -> Vec<Block> {
    update_candid_as::<_, (Vec<Block>,)>(
        pic,
        BOB_CANISTER_ID,
        Principal::anonymous(),
        "get_latest_blocks",
        ((),),
    )
    .unwrap()
    .0
}

pub(crate) fn mine_block_with_round_length(pic: &PocketIc, step: std::time::Duration) {
    let old_stats = get_stats(pic);

    loop {
        pic.advance_time(step);
        pic.tick();
        let new_stats = get_stats(pic);
        if new_stats.block_count > old_stats.block_count {
            assert_eq!(new_stats.block_count, old_stats.block_count + 1);
            while !get_stats(pic).pending_blocks.is_empty() {
                pic.tick();
            }
            break;
        }
    }
}

pub(crate) fn mine_block(pic: &PocketIc) {
    mine_block_with_round_length(pic, std::time::Duration::from_secs(60));
}

pub(crate) fn bob_balance(pic: &PocketIc, user_id: Principal) -> u64 {
    update_candid_as::<_, (Nat,)>(
        pic,
        BOB_LEDGER_CANISTER_ID,
        user_id,
        "icrc1_balance_of",
        (Account {
            owner: user_id,
            subaccount: None,
        },),
    )
    .unwrap()
    .0
     .0
    .try_into()
    .unwrap()
}

pub(crate) fn get_member_cycles(
    pic: &PocketIc,
    user_id: Principal,
) -> Result<MemberCycles, String> {
    query_candid_as::<_, (Result<MemberCycles, String>,)>(
        pic,
        BOB_POOL_CANISTER_ID,
        user_id,
        "get_member_cycles",
        ((),),
    )
    .unwrap()
    .0
}

pub(crate) fn set_member_block_cycles(
    pic: &PocketIc,
    user_id: Principal,
    block_cycles: u128,
) -> Result<(), String> {
    let block_cycles_nat: Nat = block_cycles.into();
    update_candid_as::<_, (Result<(), String>,)>(
        pic,
        BOB_POOL_CANISTER_ID,
        user_id,
        "set_member_block_cycles",
        ((block_cycles_nat),),
    )
    .unwrap()
    .0
}

pub(crate) fn get_pool_state(pic: &PocketIc) -> PoolState {
    query_candid_as::<_, (PoolState,)>(
        pic,
        BOB_POOL_CANISTER_ID,
        Principal::anonymous(),
        "get_pool_state",
        ((),),
    )
    .unwrap()
    .0
}

pub(crate) fn get_miner(pic: &PocketIc) -> Option<Principal> {
    get_pool_state(pic).miner
}

pub(crate) fn is_pool_ready(pic: &PocketIc) -> bool {
    get_miner(pic).is_some()
}

pub(crate) fn pool_logs(pic: &PocketIc, user_id: Principal) -> Vec<CanisterLogRecord> {
    pic.fetch_canister_logs(BOB_POOL_CANISTER_ID, user_id)
        .unwrap()
}

pub(crate) fn update_miner_block_cycles(
    pic: &PocketIc,
    user_id: Principal,
    miner_id: Principal,
    block_cycles: u128,
) {
    let miner_settings_args = MinerSettings {
        max_cycles_per_round: Some(block_cycles),
        new_owner: None,
    };
    update_candid_as::<_, ((),)>(
        pic,
        miner_id,
        user_id,
        "update_miner_settings",
        ((miner_settings_args),),
    )
    .unwrap();
}

pub(crate) fn get_member_rewards(pic: &PocketIc, user_id: Principal) -> Vec<Reward> {
    query_candid_as::<_, (Result<Vec<Reward>, String>,)>(
        pic,
        BOB_POOL_CANISTER_ID,
        user_id,
        "get_member_rewards",
        ((),),
    )
    .unwrap()
    .0
    .unwrap()
}

pub(crate) fn pay_member_rewards(pic: &PocketIc, user_id: Principal) {
    update_candid_as::<_, (Result<(), String>,)>(
        pic,
        BOB_POOL_CANISTER_ID,
        user_id,
        "pay_member_rewards",
        ((),),
    )
    .unwrap()
    .0
    .unwrap();
}

pub(crate) fn ensure_member_rewards(pic: &PocketIc, user_id: Principal, num_rewards: usize) {
    while get_member_rewards(pic, user_id).len() < num_rewards {
        pic.advance_time(std::time::Duration::from_secs(5));
        pic.tick();
    }
    pay_member_rewards(pic, user_id);
}

pub(crate) fn cycles_to_e8s(amount: u128) -> u64 {
    (amount / XDR_PERMYRIAD_PER_ICP as u128 + 1)
        .try_into()
        .unwrap()
}

pub(crate) fn spawn_pool_miner(pic: &PocketIc, user_id: Principal) -> Result<(), String> {
    let spawn_miners_args: Option<u64> = None;
    let bytes = Encode!(&spawn_miners_args).unwrap();
    let msg_id = pic
        .submit_call(BOB_POOL_CANISTER_ID, user_id, "spawn_miner", bytes)
        .unwrap();
    let res = loop {
        if let Some(res) = pic.ingress_status(msg_id.clone()) {
            break res;
        }
        pic.advance_time(std::time::Duration::from_secs(1));
        pic.tick();
    };
    res.map(|res| match res {
        WasmResult::Reply(_) => (),
        WasmResult::Reject(msg) => panic!("Unexpected reject: {}", msg),
    })
    .map_err(|err| err.description)
}

pub(crate) fn start_pool(pic: &PocketIc, user_id: Principal) -> Result<(), String> {
    update_candid_as::<_, (Result<(), String>,)>(pic, BOB_POOL_CANISTER_ID, user_id, "start", ((),))
        .map_err(|err| match err {
            CallError::UserError(err) => err.description,
            CallError::Reject(msg) => panic!("Unexpected reject: {}", msg),
        })?
        .0
}

pub(crate) fn stop_pool(pic: &PocketIc, user_id: Principal) -> Result<(), String> {
    update_candid_as::<_, (Result<(), String>,)>(pic, BOB_POOL_CANISTER_ID, user_id, "stop", ((),))
        .map_err(|err| match err {
            CallError::UserError(err) => err.description,
            CallError::Reject(msg) => panic!("Unexpected reject: {}", msg),
        })?
        .0
}

pub(crate) fn wait_for_stopped_pool(pic: &PocketIc) {
    loop {
        if let PoolRunningState::Stopped = get_pool_state(pic).running_state {
            break;
        }
        pic.advance_time(std::time::Duration::from_secs(5));
        pic.tick();
    }
}
