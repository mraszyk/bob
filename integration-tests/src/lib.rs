#![cfg(test)]

mod setup;
mod utils;

use crate::setup::{deploy_pool, setup};
use crate::utils::{
    bob_balance, get_remaining_pool_cycles, is_pool_ready, join_native_pool, join_pool, mine_block,
    spawn_miner, transfer_to_principal, transfer_topup_pool,
};
use candid::{Nat, Principal};
use pocket_ic::{update_candid_as, CallError};
use std::time::Duration;

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
fn test_spawn_miner() {
    let user_id = Principal::from_slice(&[0xFF; 29]);
    let pic = setup(vec![user_id]);

    let _miner_id = spawn_miner(&pic, user_id, 100_000_000);

    assert_eq!(bob_balance(&pic, user_id), 0_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 60_000_000_000_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 120_000_000_000_u64);
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
}

fn assert_pool_not_ready_error(err: CallError) {
    match err {
        CallError::UserError(user_error) => {
            assert!(user_error
                .description
                .contains("assertion failed: is_ready()"));
        }
        CallError::Reject(error_msg) => panic!("Unexpected reject: {}", error_msg),
    }
}

#[test]
fn test_pool_not_ready() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let pic = setup(vec![admin]);

    deploy_pool(&pic, admin);
    assert!(!is_pool_ready(&pic));

    let err = update_candid_as::<_, (Option<Nat>,)>(
        &pic,
        BOB_POOL_CANISTER_ID,
        admin,
        "get_remaining_cycles",
        ((),),
    )
    .unwrap_err();
    assert_pool_not_ready_error(err);

    let block_index = transfer_topup_pool(&pic, admin, 100_000_000);
    let err = update_candid_as::<_, (Option<Nat>,)>(
        &pic,
        BOB_POOL_CANISTER_ID,
        admin,
        "join_pool",
        ((block_index),),
    )
    .unwrap_err();
    assert_pool_not_ready_error(err);
}

#[test]
fn test_join_pool() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user_1 = Principal::from_slice(&[0xFE; 29]);
    let user_2 = Principal::from_slice(&[0xFD; 29]);
    let pic = setup(vec![admin, user_1, user_2]);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_pool(&pic, admin);
    assert!(!is_pool_ready(&pic));
    while !is_pool_ready(&pic) {
        pic.advance_time(Duration::from_secs(1));
        pic.tick();
    }

    for user in [admin, user_1, user_2] {
        assert_eq!(get_remaining_pool_cycles(&pic, user), None);
        assert!(join_pool(&pic, user, 10_000_000)
            .unwrap_err()
            .contains("amount too low"));
        assert_eq!(get_remaining_pool_cycles(&pic, user), None);
        join_pool(&pic, user, 100_000_000).unwrap();
        assert_eq!(
            get_remaining_pool_cycles(&pic, user),
            Some(7_800_000_000_000)
        );
        join_pool(&pic, user, 100_000_000).unwrap();
        assert_eq!(
            get_remaining_pool_cycles(&pic, user),
            Some(7_800_000_000_000 * 2)
        );
    }
}
