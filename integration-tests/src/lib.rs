#![cfg(test)]

mod setup;
mod utils;

use crate::setup::{deploy_pool, deploy_ready_pool, setup, upgrade_pool};
use crate::utils::{
    bob_balance, ensure_member_rewards, get_latest_blocks, get_member_cycles, get_member_rewards,
    get_miner, is_pool_ready, join_native_pool, join_pool, mine_block,
    mine_block_with_round_length, pool_logs, set_member_block_cycles, spawn_miner,
    transfer_to_principal, transfer_topup_pool, update_miner_block_cycles, upgrade_miner,
};
use bob_pool::MemberCycles;
use candid::{Decode, Encode, Principal};
use pocket_ic::{query_candid_as, update_candid_as, CallError, UserError, WasmResult};

// System canister IDs

pub(crate) const NNS_GOVERNANCE_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 1, 1]);
pub(crate) const NNS_ICP_LEDGER_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 2, 1, 1]);
pub(crate) const NNS_ROOT_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 3, 1, 1]);
pub(crate) const NNS_CYCLES_MINTING_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 4, 1, 1]);
pub(crate) const NNS_ICP_INDEX_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 0xB, 1, 1]);

// BoB canister IDs

pub(crate) const BOB_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x02, 0x40, 0x00, 0x55, 0x01, 0x01]);
pub(crate) const BOB_LEDGER_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x02, 0x40, 0x00, 0x59, 0x01, 0x01]);

// TODO
pub(crate) const BOB_POOL_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x02, 0x40, 0x00, 0x60, 0x01, 0x01]);

// Test scenarios

#[test]
fn test_spawn_upgrade_miner() {
    let user_id = Principal::from_slice(&[0xFF; 29]);
    let pic = setup(vec![user_id]);

    let miner_id = spawn_miner(&pic, user_id, 100_000_000);

    assert_eq!(bob_balance(&pic, user_id), 0_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 60_000_000_000_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 120_000_000_000_u64);

    let miner_cycles_before_upgrade = pic.cycle_balance(miner_id);
    upgrade_miner(&pic, user_id, miner_id);
    let miner_cycles = pic.cycle_balance(miner_id);
    let upgrade_cycles = miner_cycles_before_upgrade - miner_cycles;
    assert!(upgrade_cycles <= 3_000_000_000);

    assert_eq!(bob_balance(&pic, user_id), 120_000_000_000_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 180_000_000_000_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 240_000_000_000_u64);
}

#[test]
fn test_update_miner_block_cycles() {
    let user_1 = Principal::from_slice(&[0xFF; 29]);
    let user_2 = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![user_1, user_2]);

    let miner_1 = spawn_miner(&pic, user_1, 100_000_000);
    let miner_2 = spawn_miner(&pic, user_2, 100_000_000);

    let miner_1_cycles = 50_000_000_000;
    let miner_2_cycles = 100_000_000_000;
    update_miner_block_cycles(&pic, user_1, miner_1, miner_1_cycles);
    update_miner_block_cycles(&pic, user_2, miner_2, miner_2_cycles);

    mine_block(&pic);

    let blocks = get_latest_blocks(&pic);
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].total_cycles_burned.unwrap() as u128,
        miner_1_cycles + miner_2_cycles
    );
}

#[test]
fn test_native_pool() {
    let user_1 = Principal::from_slice(&[0xFF; 29]);
    let user_2 = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![user_1, user_2]);

    join_native_pool(&pic, user_1, 100_000_000).unwrap();
    join_native_pool(&pic, user_2, 200_000_000).unwrap();

    assert_eq!(bob_balance(&pic, user_1), 0_u64);
    assert_eq!(bob_balance(&pic, user_2), 0_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_1), 30_000_000_000_u64);
    assert_eq!(bob_balance(&pic, user_2), 30_000_000_000_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_1), 60_000_000_000_u64);
    assert_eq!(bob_balance(&pic, user_2), 60_000_000_000_u64);
}

// Test pool

#[test]
fn test_pool_not_ready() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let pic = setup(vec![admin]);

    deploy_pool(&pic, admin);
    assert!(!is_pool_ready(&pic));

    let err = query_candid_as::<_, (Result<Option<MemberCycles>, String>,)>(
        &pic,
        BOB_POOL_CANISTER_ID,
        admin,
        "get_member_cycles",
        ((),),
    )
    .unwrap()
    .0
    .unwrap_err();
    assert!(err.contains("BoB pool canister is not ready; please try again later."));

    let block_index = transfer_topup_pool(&pic, admin, 100_000_000);
    let err = update_candid_as::<_, (Result<(), String>,)>(
        &pic,
        BOB_POOL_CANISTER_ID,
        admin,
        "join_pool",
        ((block_index),),
    )
    .unwrap()
    .0
    .unwrap_err();
    assert!(err.contains("BoB pool canister is not ready; please try again later."));

    assert_eq!(pool_logs(&pic, admin).len(), 1);
    assert!(String::from_utf8(pool_logs(&pic, admin)[0].content.clone()).unwrap().contains("[TRAP]: Failed to init: Error from ICP ledger canister: the debit account doesn't have enough funds to complete the transaction, current balance: 0.00000000"));
}

#[test]
fn test_failed_upgrade_pool() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let pic = setup(vec![admin]);

    deploy_pool(&pic, admin);
    assert!(!is_pool_ready(&pic));
    assert!(get_miner(&pic).is_none());

    // avoid rate-limiting errors due to frequent code installation
    for _ in 0..10 {
        pic.tick();
    }

    let err = upgrade_pool(&pic, admin).unwrap_err();
    match err {
        CallError::Reject(msg) => panic!("Unexpected reject: {}", msg),
        CallError::UserError(err) => assert!(err.description.contains("No miner found.")),
    };

    assert_eq!(pool_logs(&pic, admin).len(), 2);
    assert!(String::from_utf8(pool_logs(&pic, admin)[0].content.clone())
        .unwrap()
        .contains("Failed to init: Error from ICP ledger canister: the debit account doesn't have enough funds to complete the transaction, current balance: 0.00000000\n"));
    assert!(String::from_utf8(pool_logs(&pic, admin)[1].content.clone())
        .unwrap()
        .contains("No miner found."));
}

#[test]
fn test_join_pool() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user_1 = Principal::from_slice(&[0xFE; 29]);
    let user_2 = Principal::from_slice(&[0xFD; 29]);
    let pic = setup(vec![admin, user_1, user_2]);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    for user in [admin, user_1, user_2] {
        assert!(get_member_cycles(&pic, user).is_none());
        assert!(join_pool(&pic, user, 10_000_000).unwrap_err().contains(
            "Transaction amount (0.10000000 Token) too low: should be at least 0.99990000 Token."
        ));
        assert!(get_member_cycles(&pic, user).is_none());
        join_pool(&pic, user, 100_000_000).unwrap();
        let member_cycles = get_member_cycles(&pic, user).unwrap();
        assert_eq!(member_cycles.block, 0);
        assert_eq!(member_cycles.remaining, 7_800_000_000_000);
        join_pool(&pic, user, 100_000_000).unwrap();
        let member_cycles = get_member_cycles(&pic, user).unwrap();
        assert_eq!(member_cycles.block, 0);
        assert_eq!(member_cycles.remaining, 2 * 7_800_000_000_000);
    }

    assert_eq!(pool_logs(&pic, admin).len(), 1);
    assert!(String::from_utf8(pool_logs(&pic, admin)[0].content.clone())
        .unwrap()
        .contains("Sent BoB top up transfer at ICP ledger block index 4."));
}

#[test]
fn test_upgrade_pool() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let pic = setup(vec![admin]);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);
    let bob_miner = get_miner(&pic).unwrap();

    join_pool(&pic, admin, 100_000_000).unwrap();
    set_member_block_cycles(&pic, admin, 100_000_000_000_u128).unwrap();
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 100_000_000_000);
    assert_eq!(member_cycles.remaining, 7_800_000_000_000);

    upgrade_pool(&pic, admin).unwrap();
    assert!(is_pool_ready(&pic));
    assert_eq!(get_miner(&pic).unwrap(), bob_miner);
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 100_000_000_000);
    assert_eq!(member_cycles.remaining, 7_800_000_000_000);

    assert_eq!(pool_logs(&pic, admin).len(), 1);
    assert!(String::from_utf8(pool_logs(&pic, admin)[0].content.clone())
        .unwrap()
        .contains("Sent BoB top up transfer at ICP ledger block index 2."));
}

#[test]
fn test_set_member_block_cycles() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let pic = setup(vec![admin]);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    let err = set_member_block_cycles(&pic, admin, 100_000_000_000_u128).unwrap_err();
    assert!(err.contains(&format!("The caller {} is no pool member.", admin)));

    join_pool(&pic, admin, 100_000_000).unwrap();

    set_member_block_cycles(&pic, admin, 100_000_000_000_u128).unwrap();

    let err = set_member_block_cycles(&pic, admin, 100_000_000_001_u128).unwrap_err();
    assert!(err.contains("The number of block cycles 100000000001 is not a multiple of 1000000."));
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 100_000_000_000_u128);

    set_member_block_cycles(&pic, admin, 15_000_000_000_u128).unwrap();
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 15_000_000_000_u128);

    let err = set_member_block_cycles(&pic, admin, 14_000_000_000_u128).unwrap_err();
    assert!(err.contains("The number of block cycles 14000000000 is too small."));
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 15_000_000_000_u128);

    set_member_block_cycles(&pic, admin, 0_u128).unwrap();
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 0_u128);

    set_member_block_cycles(&pic, admin, 15_000_000_000_u128).unwrap();
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 15_000_000_000_u128);

    assert_eq!(pool_logs(&pic, admin).len(), 1);
    assert!(String::from_utf8(pool_logs(&pic, admin)[0].content.clone())
        .unwrap()
        .contains("Sent BoB top up transfer at ICP ledger block index 2."));
}

#[test]
fn test_pool_inactive_by_default() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![admin, user]);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    let miner = spawn_miner(&pic, user, 100_000_000);

    let miner_cycles = 50_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);

    let num_blocks = 3;
    for _ in 0..num_blocks {
        mine_block(&pic);
    }

    let blocks = get_latest_blocks(&pic);
    assert_eq!(blocks.len(), num_blocks);
    for block in blocks {
        assert!((miner_cycles..=2 * miner_cycles)
            .contains(&(block.total_cycles_burned.unwrap() as u128)));
    }

    assert_eq!(pool_logs(&pic, admin).len(), 1);
    assert!(String::from_utf8(pool_logs(&pic, admin)[0].content.clone())
        .unwrap()
        .contains("Sent BoB top up transfer at ICP ledger block index 3."));
}

#[test]
fn test_pool_rewards() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user_1 = Principal::from_slice(&[0xFE; 29]);
    let user_2 = Principal::from_slice(&[0xFD; 29]);
    let user = Principal::from_slice(&[0xFC; 29]);
    let pic = setup(vec![admin, user_1, user_2, user]);

    let miner = spawn_miner(&pic, user, 100_000_000);
    let miner_cycles = 15_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);
    mine_block_with_round_length(&pic, std::time::Duration::from_secs(5));

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    join_pool(&pic, admin, 1_000_000_000).unwrap();
    join_pool(&pic, user_1, 1_000_000_000).unwrap();
    join_pool(&pic, user_2, 1_000_000_000).unwrap();

    let admin_block_cycles = 3_000_000_000_000;
    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();
    let user_1_block_cycles = 4_000_000_000_000;
    set_member_block_cycles(&pic, user_1, user_1_block_cycles).unwrap();
    let user_2_block_cycles = 5_000_000_000_000;
    set_member_block_cycles(&pic, user_2, user_2_block_cycles).unwrap();

    let member_cycles_admin = get_member_cycles(&pic, admin).unwrap();
    let member_cycles_user_1 = get_member_cycles(&pic, user_1).unwrap();
    let member_cycles_user_2 = get_member_cycles(&pic, user_2).unwrap();

    let num_blocks = 3;
    for _ in 0..num_blocks {
        mine_block_with_round_length(&pic, std::time::Duration::from_secs(5));
    }

    ensure_member_rewards(&pic, admin, num_blocks);
    ensure_member_rewards(&pic, user_1, num_blocks);
    ensure_member_rewards(&pic, user_2, num_blocks);

    let blocks = get_latest_blocks(&pic);
    assert_eq!(blocks.len(), num_blocks + 2);
    let total_block_cycles = admin_block_cycles + user_1_block_cycles + user_2_block_cycles;
    for block in blocks.iter().take(num_blocks) {
        assert!(
            (total_block_cycles + miner_cycles..=total_block_cycles + 2 * miner_cycles)
                .contains(&(block.total_cycles_burned.unwrap() as u128))
        );
    }
    for block in blocks.iter().skip(num_blocks) {
        assert!((miner_cycles..=2 * miner_cycles)
            .contains(&(block.total_cycles_burned.unwrap() as u128)));
    }

    assert_eq!(
        bob_balance(&pic, admin) as u128,
        (60_000_000_000 - 3_000_000) * admin_block_cycles * num_blocks as u128 / total_block_cycles
    );
    assert_eq!(
        bob_balance(&pic, user_1) as u128,
        (60_000_000_000 - 3_000_000) * user_1_block_cycles * num_blocks as u128
            / total_block_cycles
    );
    assert_eq!(
        bob_balance(&pic, user_2) as u128,
        (60_000_000_000 - 3_000_000) * user_2_block_cycles * num_blocks as u128
            / total_block_cycles
    );

    let max_pool_fee = 5_000_000_000;
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, admin_block_cycles);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining + (admin_block_cycles + max_pool_fee / 3) * num_blocks as u128,
        member_cycles_admin.remaining
    );
    let member_cycles = get_member_cycles(&pic, user_1).unwrap();
    assert_eq!(member_cycles.block, user_1_block_cycles);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining + (user_1_block_cycles + max_pool_fee / 3) * num_blocks as u128,
        member_cycles_user_1.remaining
    );
    let member_cycles = get_member_cycles(&pic, user_2).unwrap();
    assert_eq!(member_cycles.block, user_2_block_cycles);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining + (user_2_block_cycles + max_pool_fee / 3) * num_blocks as u128,
        member_cycles_user_2.remaining
    );

    assert_eq!(pool_logs(&pic, admin).len(), 1);
    assert!(String::from_utf8(pool_logs(&pic, admin)[0].content.clone())
        .unwrap()
        .contains("Sent BoB top up transfer at ICP ledger block index 7."));
}

#[test]
fn test_simultaneous_reward_payments() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![admin, user]);

    let miner = spawn_miner(&pic, user, 100_000_000);
    let miner_cycles = 15_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);
    mine_block_with_round_length(&pic, std::time::Duration::from_secs(5));

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    join_pool(&pic, admin, 1_000_000_000).unwrap();

    let admin_block_cycles = 3_000_000_000_000;
    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();

    mine_block_with_round_length(&pic, std::time::Duration::from_secs(5));

    while get_member_rewards(&pic, admin).is_empty() {
        pic.advance_time(std::time::Duration::from_secs(1));
        pic.tick();
    }

    let msg_1 = pic
        .submit_call(
            BOB_POOL_CANISTER_ID,
            admin,
            "pay_member_rewards",
            Encode!(&()).unwrap(),
        )
        .unwrap();
    let msg_2 = pic
        .submit_call(
            BOB_POOL_CANISTER_ID,
            admin,
            "pay_member_rewards",
            Encode!(&()).unwrap(),
        )
        .unwrap();

    let unwrap_res = |res: Result<WasmResult, UserError>| match res {
        Ok(WasmResult::Reply(data)) => Decode!(&data, Result<(), String>).unwrap(),
        Ok(WasmResult::Reject(msg)) => panic!("Unexpected reject: {}", msg),
        Err(err) => panic!("Unexpected error: {}", err.description),
    };
    let res_1 = unwrap_res(pic.await_call(msg_1));
    let res_2 = unwrap_res(pic.await_call(msg_2));

    assert!(res_1.is_err() || res_2.is_err());
    if let Err(err) = res_1 {
        assert!(err.contains("Concurrency error"));
    }
    if let Err(err) = res_2 {
        assert!(err.contains("Concurrency error"));
    }
}
