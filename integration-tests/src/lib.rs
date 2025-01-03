#![cfg(test)]

mod setup;
mod utils;

use crate::setup::{deploy_ready_pool, setup, upgrade_pool, XDR_PERMYRIAD_PER_ICP};
use crate::utils::{
    bob_balance, check_pool_logs, cycles_to_e8s, ensure_member_rewards, get_latest_blocks,
    get_member_cycles, get_member_rewards, get_miner, get_pool_state, is_pool_ready,
    join_native_pool, join_pool, mine_block, mine_block_with_round_length, pool_logs,
    set_member_block_cycles, spawn_miner, start_pool, stop_pool, transfer_to_principal,
    update_miner_block_cycles, upgrade_miner, wait_for_stopped_pool,
};
use bob_pool::{MemberCycles, PoolRunningState, BOB_POOL_BLOCK_FEE};
use candid::{Decode, Encode, Principal};
use pocket_ic::management_canister::CanisterSettings;
use pocket_ic::{UserError, WasmResult};

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
fn test_join_pool() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user_1 = Principal::from_slice(&[0xFE; 29]);
    let user_2 = Principal::from_slice(&[0xFD; 29]);
    let pic = setup(vec![admin, user_1, user_2]);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    for user in [admin, user_1, user_2] {
        assert!(get_member_cycles(&pic, user)
            .unwrap_err()
            .contains(&format!("The caller {} is no pool member.", user)));
        assert!(join_pool(&pic, user, 10_000_000).unwrap_err().contains(
            "Transaction amount (0.10000000 Token) too low: should be at least 0.99990000 Token."
        ));
        assert!(get_member_cycles(&pic, user)
            .unwrap_err()
            .contains(&format!("The caller {} is no pool member.", user)));
        join_pool(&pic, user, 100_000_000).unwrap();
        let member_cycles = get_member_cycles(&pic, user).unwrap();
        assert_eq!(member_cycles.block, 0);
        assert_eq!(
            member_cycles.remaining,
            XDR_PERMYRIAD_PER_ICP as u128 * 100_000_000
        );
        join_pool(&pic, user, 100_000_000).unwrap();
        let member_cycles = get_member_cycles(&pic, user).unwrap();
        assert_eq!(member_cycles.block, 0);
        assert_eq!(
            member_cycles.remaining,
            2 * XDR_PERMYRIAD_PER_ICP as u128 * 100_000_000
        );
    }

    check_pool_logs(&pic, admin);
}

#[test]
fn test_upgrade_pool() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![admin, user]);

    let miner = spawn_miner(&pic, user, 1_000_000_000);
    let miner_cycles = 50_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);
    let bob_miner = get_miner(&pic).unwrap();

    let admin_block_cycles = 50_000_000_000_000;
    let join_e8s = cycles_to_e8s(admin_block_cycles + BOB_POOL_BLOCK_FEE);
    join_pool(&pic, admin, join_e8s).unwrap();

    let check_member_cycles = |member_cycles: MemberCycles| {
        assert_eq!(member_cycles.block, admin_block_cycles);
        assert!((admin_block_cycles + BOB_POOL_BLOCK_FEE
            ..admin_block_cycles + 2 * BOB_POOL_BLOCK_FEE)
            .contains(&member_cycles.remaining));
    };

    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();
    check_member_cycles(get_member_cycles(&pic, admin).unwrap());

    ensure_member_rewards(&pic, admin, 1);

    let err = start_pool(&pic, user).unwrap_err();
    assert!(err.contains("The method `start` can only be called by controllers."));
    let err = stop_pool(&pic, user).unwrap_err();
    assert!(err.contains("The method `stop` can only be called by controllers."));

    match get_pool_state(&pic).running_state {
        PoolRunningState::Running => (),
        running_state => panic!("Unexpected pool running state: {:?}", running_state),
    };
    stop_pool(&pic, admin).unwrap();
    match get_pool_state(&pic).running_state {
        PoolRunningState::Stopping => (),
        running_state => panic!("Unexpected pool running state: {:?}", running_state),
    };
    wait_for_stopped_pool(&pic);
    match get_pool_state(&pic).running_state {
        PoolRunningState::Stopped => (),
        running_state => panic!("Unexpected pool running state: {:?}", running_state),
    };
    start_pool(&pic, admin).unwrap();
    match get_pool_state(&pic).running_state {
        PoolRunningState::Running => (),
        running_state => panic!("Unexpected pool running state: {:?}", running_state),
    };
    stop_pool(&pic, admin).unwrap();
    wait_for_stopped_pool(&pic);
    match get_pool_state(&pic).running_state {
        PoolRunningState::Stopped => (),
        running_state => panic!("Unexpected pool running state: {:?}", running_state),
    };

    join_pool(&pic, admin, join_e8s).unwrap();
    check_member_cycles(get_member_cycles(&pic, admin).unwrap());

    upgrade_pool(&pic, admin).unwrap();
    assert!(is_pool_ready(&pic));
    assert_eq!(get_miner(&pic).unwrap(), bob_miner);
    check_member_cycles(get_member_cycles(&pic, admin).unwrap());

    ensure_member_rewards(&pic, admin, 2);

    join_pool(&pic, admin, join_e8s).unwrap();
    check_member_cycles(get_member_cycles(&pic, admin).unwrap());

    check_pool_logs(&pic, admin);
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
    assert!(err.contains(
        "The number of block cycles 14000000000 is less than the minimum of 15000000000."
    ));
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 15_000_000_000_u128);

    set_member_block_cycles(&pic, admin, 0_u128).unwrap();
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 0_u128);

    set_member_block_cycles(&pic, admin, 15_000_000_000_u128).unwrap();
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 15_000_000_000_u128);

    check_pool_logs(&pic, admin);
}

#[test]
fn test_pool_inactive_by_default() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![admin, user]);

    let miner = spawn_miner(&pic, user, 1_000_000_000);
    let miner_cycles = 50_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    let pool_cycles = pic.cycle_balance(BOB_POOL_CANISTER_ID);
    let num_blocks = 8;
    for _ in 0..num_blocks {
        mine_block(&pic);
    }
    let pool_cycles_consumption = pool_cycles - pic.cycle_balance(BOB_POOL_CANISTER_ID);
    let pool_cycles_consumption_per_block = pool_cycles_consumption / num_blocks as u128;
    assert!(pool_cycles_consumption_per_block < 100_000_000);

    let blocks = get_latest_blocks(&pic);
    assert!(blocks.len() < 10);
    assert_eq!(blocks.len(), num_blocks);
    for block in blocks {
        assert!((miner_cycles..=3 * miner_cycles)
            .contains(&(block.total_cycles_burned.unwrap() as u128)));
    }

    check_pool_logs(&pic, admin);
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

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    let pool_miner = get_miner(&pic).unwrap();
    let pool_miner_cycles = pic.cycle_balance(pool_miner);
    let pool_miner_extra_cycles = pool_miner_cycles - 1_000_000_000_000;

    let pool_state = get_pool_state(&pic);
    assert_eq!(pool_state.num_active_members, 0);
    assert_eq!(pool_state.total_active_member_block_cycles, 0);
    assert_eq!(pool_state.total_cycles_burnt, 0);
    assert_eq!(pool_state.total_bob_rewards, 0);

    let num_blocks = 3;
    let admin_block_cycles = 30_000_000_000_000;
    let user_1_block_cycles = 40_000_000_000_000;
    let user_2_block_cycles = 50_000_000_000_000;
    let total_block_cycles = admin_block_cycles + user_1_block_cycles + user_2_block_cycles;
    join_pool(
        &pic,
        admin,
        cycles_to_e8s((admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128),
    )
    .unwrap();
    join_pool(
        &pic,
        user_1,
        cycles_to_e8s((user_1_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128),
    )
    .unwrap();
    join_pool(
        &pic,
        user_2,
        cycles_to_e8s((user_2_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128),
    )
    .unwrap();

    let pool_state = get_pool_state(&pic);
    assert_eq!(pool_state.num_active_members, 0);
    assert_eq!(pool_state.total_active_member_block_cycles, 0);
    assert_eq!(pool_state.total_cycles_burnt, 0);
    assert_eq!(pool_state.total_bob_rewards, 0);

    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();
    set_member_block_cycles(&pic, user_1, user_1_block_cycles).unwrap();
    set_member_block_cycles(&pic, user_2, user_2_block_cycles).unwrap();

    let pool_state = get_pool_state(&pic);
    assert_eq!(pool_state.num_active_members, 3);
    assert_eq!(pool_state.total_active_member_block_cycles, total_block_cycles);
    assert_eq!(pool_state.total_cycles_burnt, 0);
    assert_eq!(pool_state.total_bob_rewards, 0);

    let member_cycles_admin = get_member_cycles(&pic, admin).unwrap();
    let member_cycles_user_1 = get_member_cycles(&pic, user_1).unwrap();
    let member_cycles_user_2 = get_member_cycles(&pic, user_2).unwrap();

    let pool_cycles = pic.cycle_balance(BOB_POOL_CANISTER_ID);

    ensure_member_rewards(&pic, admin, num_blocks - 1);
    ensure_member_rewards(&pic, user_1, num_blocks - 1);
    ensure_member_rewards(&pic, user_2, num_blocks - 1);

    let pool_state = get_pool_state(&pic);
    assert_eq!(pool_state.num_active_members, 3);
    assert_eq!(pool_state.total_active_member_block_cycles, total_block_cycles);
    assert_eq!(pool_state.total_cycles_burnt, total_block_cycles * (num_blocks as u128 - 1));
    assert_eq!(pool_state.total_bob_rewards, (60_000_000_000 - 3_000_000) * (num_blocks as u128 - 1));

    ensure_member_rewards(&pic, admin, num_blocks);
    ensure_member_rewards(&pic, user_1, num_blocks);
    ensure_member_rewards(&pic, user_2, num_blocks);

    let pool_state = get_pool_state(&pic);
    assert_eq!(pool_state.num_active_members, 0);
    assert_eq!(pool_state.total_active_member_block_cycles, 0);
    assert_eq!(pool_state.total_cycles_burnt, total_block_cycles * num_blocks as u128);
    assert_eq!(pool_state.total_bob_rewards, (60_000_000_000 - 3_000_000) * num_blocks as u128);

    let pool_cycles_consumption = pool_cycles - pic.cycle_balance(BOB_POOL_CANISTER_ID);
    let pool_cycles_consumption_per_block = (pool_miner_extra_cycles + pool_cycles_consumption)
        / num_blocks as u128
        - total_block_cycles;
    assert!(pool_cycles_consumption_per_block <= BOB_POOL_BLOCK_FEE);

    let mut blocks = get_latest_blocks(&pic);
    assert!(blocks.len() < 10);
    blocks.reverse();
    assert_eq!(blocks.len(), num_blocks + 2);
    for (idx, block) in blocks.into_iter().enumerate() {
        if idx >= 2 {
            assert!(
                (total_block_cycles + miner_cycles..=total_block_cycles + 3 * miner_cycles)
                    .contains(&(block.total_cycles_burned.unwrap() as u128))
            );
        } else {
            assert!((miner_cycles..=3 * miner_cycles)
                .contains(&(block.total_cycles_burned.unwrap() as u128)));
        }
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

    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, admin_block_cycles);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining
            + (admin_block_cycles + BOB_POOL_BLOCK_FEE / 3) * num_blocks as u128,
        member_cycles_admin.remaining
    );
    let member_cycles = get_member_cycles(&pic, user_1).unwrap();
    assert_eq!(member_cycles.block, user_1_block_cycles);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining
            + (user_1_block_cycles + BOB_POOL_BLOCK_FEE / 3) * num_blocks as u128,
        member_cycles_user_1.remaining
    );
    let member_cycles = get_member_cycles(&pic, user_2).unwrap();
    assert_eq!(member_cycles.block, user_2_block_cycles);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining
            + (user_2_block_cycles + BOB_POOL_BLOCK_FEE / 3) * num_blocks as u128,
        member_cycles_user_2.remaining
    );

    check_pool_logs(&pic, admin);
}

#[test]
fn test_pool_member_interrupt() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user_1 = Principal::from_slice(&[0xFE; 29]);
    let user_2 = Principal::from_slice(&[0xFD; 29]);
    let user = Principal::from_slice(&[0xFC; 29]);
    let pic = setup(vec![admin, user_1, user_2, user]);

    let miner = spawn_miner(&pic, user, 100_000_000);
    let miner_cycles = 15_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    let num_blocks = 2;
    let admin_block_cycles = 30_000_000_000_000;
    let total_block_cycles = admin_block_cycles;
    join_pool(
        &pic,
        admin,
        cycles_to_e8s(2 * (admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128),
    )
    .unwrap();
    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();

    let member_cycles_admin = get_member_cycles(&pic, admin).unwrap();

    ensure_member_rewards(&pic, admin, num_blocks);

    set_member_block_cycles(&pic, admin, 0).unwrap();

    for _ in 0..num_blocks {
        mine_block_with_round_length(&pic, std::time::Duration::from_secs(5));
    }

    assert_eq!(get_member_rewards(&pic, admin).len(), num_blocks);
    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, 0);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining + (admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128,
        member_cycles_admin.remaining
    );

    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();

    ensure_member_rewards(&pic, admin, 2 * num_blocks);

    let mut blocks = get_latest_blocks(&pic);
    assert!(blocks.len() < 10);
    blocks.reverse();
    assert_eq!(blocks.len(), 3 * num_blocks + 2);
    for (idx, block) in blocks.into_iter().enumerate() {
        if idx >= 2 && ((idx - 2) / num_blocks) % 2 == 0 {
            assert!(
                (total_block_cycles + miner_cycles..=total_block_cycles + 3 * miner_cycles)
                    .contains(&(block.total_cycles_burned.unwrap() as u128))
            );
        } else {
            assert!((miner_cycles..=3 * miner_cycles)
                .contains(&(block.total_cycles_burned.unwrap() as u128)));
        }
    }

    assert_eq!(
        bob_balance(&pic, admin) as u128,
        2 * (60_000_000_000 - 1_000_000) * num_blocks as u128
    );

    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, admin_block_cycles);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining
            + 2 * (admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128,
        member_cycles_admin.remaining
    );

    check_pool_logs(&pic, admin);
}

#[test]
fn test_simultaneous_reward_payments() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![admin, user]);

    let miner = spawn_miner(&pic, user, 100_000_000);
    let miner_cycles = 15_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    let num_blocks = 1;
    let admin_block_cycles = 30_000_000_000_000;
    join_pool(
        &pic,
        admin,
        cycles_to_e8s((admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128),
    )
    .unwrap();
    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();

    while get_member_rewards(&pic, admin).is_empty() {
        pic.advance_time(std::time::Duration::from_secs(5));
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

    let member_rewards = get_member_rewards(&pic, admin);
    assert_eq!(member_rewards.len(), 1);
    assert!(member_rewards[0].bob_block_index.is_some());

    check_pool_logs(&pic, admin);
}

#[test]
fn test_join_pool_after_rewards() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![admin, user]);

    let miner = spawn_miner(&pic, user, 100_000_000);
    let miner_cycles = 15_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    let num_blocks = 1;
    let admin_block_cycles = 30_000_000_000_000;
    join_pool(
        &pic,
        admin,
        cycles_to_e8s((admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128),
    )
    .unwrap();
    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();

    ensure_member_rewards(&pic, admin, num_blocks);

    let admin_rewards = get_member_rewards(&pic, admin);
    join_pool(
        &pic,
        admin,
        cycles_to_e8s((admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128),
    )
    .unwrap();
    assert_eq!(get_member_rewards(&pic, admin).len(), admin_rewards.len());

    ensure_member_rewards(&pic, admin, num_blocks);

    check_pool_logs(&pic, admin);
}

#[test]
fn test_frozen_bob() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![admin, user]);

    let miner = spawn_miner(&pic, user, 100_000_000);
    let miner_cycles = 15_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    let num_blocks = 2;
    let admin_block_cycles = 30_000_000_000_000;
    let total_block_cycles = admin_block_cycles;
    join_pool(
        &pic,
        admin,
        cycles_to_e8s(2 * (admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128),
    )
    .unwrap();
    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();

    let member_cycles_admin = get_member_cycles(&pic, admin).unwrap();

    ensure_member_rewards(&pic, admin, num_blocks);

    pic.update_canister_settings(
        BOB_CANISTER_ID,
        Some(NNS_ROOT_CANISTER_ID),
        CanisterSettings {
            freezing_threshold: Some((1_u64 << 63).into()),
            ..Default::default()
        },
    )
    .unwrap();

    for _ in 0..180 {
        pic.advance_time(std::time::Duration::from_secs(5));
        pic.tick();
    }

    pic.update_canister_settings(
        BOB_CANISTER_ID,
        Some(NNS_ROOT_CANISTER_ID),
        CanisterSettings {
            freezing_threshold: Some((86_400_u64 * 30).into()),
            ..Default::default()
        },
    )
    .unwrap();

    for _ in 0..num_blocks {
        mine_block_with_round_length(&pic, std::time::Duration::from_secs(5));
    }

    ensure_member_rewards(&pic, admin, 2 * num_blocks);

    let mut blocks = get_latest_blocks(&pic);
    assert!(blocks.len() < 10);
    blocks.reverse();
    assert_eq!(blocks.len(), 2 + num_blocks + 1 + num_blocks);
    for (idx, block) in blocks.into_iter().enumerate() {
        if idx >= 2 && idx - 2 != num_blocks {
            assert!(
                (total_block_cycles + miner_cycles..=total_block_cycles + 3 * miner_cycles)
                    .contains(&(block.total_cycles_burned.unwrap() as u128))
            );
        } else {
            assert!((miner_cycles..=3 * miner_cycles)
                .contains(&(block.total_cycles_burned.unwrap() as u128)));
        }
    }

    assert_eq!(
        bob_balance(&pic, admin) as u128,
        2 * (60_000_000_000 - 1_000_000) * num_blocks as u128
    );

    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, admin_block_cycles);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining
            + 2 * (admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128,
        member_cycles_admin.remaining
    );

    let logs: Vec<_> = pool_logs(&pic, admin)
        .into_iter()
        .map(|log| String::from_utf8(log.content).unwrap())
        .collect();
    assert!(logs
        .iter()
        .any(|msg| msg.contains("Sent BoB top up transfer at ICP ledger block index")));
    assert!(logs
        .iter()
        .any(|msg| msg.contains("Canister 6lnhz-oaaaa-aaaas-aabkq-cai is out of cycles")));
    assert!(logs.iter().all(|msg| msg
        .contains("Sent BoB top up transfer at ICP ledger block index")
        || msg.contains("Canister 6lnhz-oaaaa-aaaas-aabkq-cai is out of cycles")));
}

#[test]
fn test_frozen_pool() {
    let admin = Principal::from_slice(&[0xFF; 29]);
    let user = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![admin, user]);

    let miner = spawn_miner(&pic, user, 100_000_000);
    let miner_cycles = 15_000_000_000;
    update_miner_block_cycles(&pic, user, miner, miner_cycles);

    transfer_to_principal(&pic, admin, BOB_POOL_CANISTER_ID, 100_010_000);
    deploy_ready_pool(&pic, admin);

    let num_blocks = 4;
    let admin_block_cycles = 30_000_000_000_000;
    let total_block_cycles = admin_block_cycles;
    join_pool(
        &pic,
        admin,
        cycles_to_e8s((admin_block_cycles + BOB_POOL_BLOCK_FEE) * num_blocks as u128),
    )
    .unwrap();
    set_member_block_cycles(&pic, admin, admin_block_cycles).unwrap();

    let member_cycles_admin = get_member_cycles(&pic, admin).unwrap();

    ensure_member_rewards(&pic, admin, 1);

    for _ in 0..40 {
        pic.advance_time(std::time::Duration::from_secs(5));
        pic.tick();
    }

    pic.update_canister_settings(
        BOB_POOL_CANISTER_ID,
        Some(admin),
        CanisterSettings {
            freezing_threshold: Some((1_u64 << 63).into()),
            ..Default::default()
        },
    )
    .unwrap();

    mine_block_with_round_length(&pic, std::time::Duration::from_secs(5));
    mine_block_with_round_length(&pic, std::time::Duration::from_secs(5));

    pic.update_canister_settings(
        BOB_POOL_CANISTER_ID,
        Some(admin),
        CanisterSettings {
            freezing_threshold: Some(0_u64.into()),
            ..Default::default()
        },
    )
    .unwrap();

    ensure_member_rewards(&pic, admin, num_blocks - 1);

    let mut blocks = get_latest_blocks(&pic);
    assert!(blocks.len() < 10);
    blocks.reverse();
    assert_eq!(blocks.len(), 2 + num_blocks + 2);
    for (idx, block) in blocks.into_iter().enumerate() {
        if idx >= 2 && !(2..=4).contains(&(idx - 2)) {
            assert!(
                (total_block_cycles + miner_cycles..=total_block_cycles + 3 * miner_cycles)
                    .contains(&(block.total_cycles_burned.unwrap() as u128))
            );
        } else {
            assert!((miner_cycles..=3 * miner_cycles)
                .contains(&(block.total_cycles_burned.unwrap() as u128)));
        }
    }

    assert_eq!(
        bob_balance(&pic, admin) as u128,
        (60_000_000_000 - 1_000_000) * (num_blocks - 1) as u128
    );

    let member_cycles = get_member_cycles(&pic, admin).unwrap();
    assert_eq!(member_cycles.block, admin_block_cycles);
    assert_eq!(member_cycles.pending, 0);
    assert_eq!(
        member_cycles.remaining
            + (admin_block_cycles + BOB_POOL_BLOCK_FEE) * (num_blocks - 1) as u128,
        member_cycles_admin.remaining
    );

    let logs: Vec<_> = pool_logs(&pic, admin)
        .into_iter()
        .map(|log| String::from_utf8(log.content).unwrap())
        .collect();
    let miner = get_miner(&pic).unwrap();
    assert!(logs
        .iter()
        .any(|msg| msg.contains("Sent BoB top up transfer at ICP ledger block index")));
    assert!(logs
        .iter()
        .any(|msg| msg.contains("WARN(stage_1): skipped blocks 1445..<1446")));
    assert!(logs
        .iter()
        .any(|msg| msg.contains(&format!("Canister {} is out of cycles", miner))));
    assert!(logs.iter().all(|msg| msg
        .contains("Sent BoB top up transfer at ICP ledger block index")
        || msg.contains("WARN(stage_1): skipped blocks 1445..<1446")
        || msg.contains(&format!("Canister {} is out of cycles", miner))
        || msg.contains("ERR(stage_3): Last cycles burned")));
}
