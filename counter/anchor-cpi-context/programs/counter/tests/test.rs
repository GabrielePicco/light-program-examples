// #![cfg(feature = "test-sbf")]

use anchor_lang::prelude::msg;
use anchor_lang::{AnchorDeserialize, Event, InstructionData, ToAccountMetas};
use counter::CounterAccount;
use light_client::indexer::{CompressedAccount, TreeInfo};
use light_compressed_account::address::derive_address;
use light_hasher::hash_to_field_size::hashv_to_bn254_field_size_be_const_array;
use light_program_test::{
    program_test::LightProgramTest, AddressWithTree, Indexer, ProgramTestConfig, Rpc, RpcError,
};
use light_sdk::instruction::{
    account_meta::{CompressedAccountMeta, CompressedAccountMetaClose},
    PackedAccounts, SystemAccountMetaConfig,
};
use light_sdk_types::address::AddressSeed;
use light_sdk_types::NOOP_PROGRAM_ID;
use solana_pubkey::Pubkey;
use solana_sdk::{
    instruction::Instruction,
    signature::{Keypair, Signature, Signer},
};

#[tokio::test]
async fn test_counter_delegation() {
    let mut config = ProgramTestConfig::new_v2(true, Some(vec![("counter", counter::ID), ("delegation", delegation::ID)]));
    config.log_light_protocol_events = true;
    let mut rpc = LightProgramTest::new(config).await.unwrap();
    let payer = rpc.get_payer().insecure_clone();

    let address_tree_info = rpc.get_address_tree_v2();
    let seed = hashv_to_bn254_field_size_be_const_array::<3>(&[
        b"counter".as_slice(),
        payer.pubkey().as_ref(),
    ])
    .unwrap();

    let address = derive_address(
        &seed,
        &address_tree_info.tree.to_bytes(),
        &counter::ID.to_bytes(),
    );

    // Create the counter.
    create_counter(&mut rpc, &payer, &address, address_tree_info)
        .await
        .unwrap();

    // Check that it was created correctly.
    let compressed_account = rpc
        .get_compressed_account(address, None)
        .await
        .unwrap()
        .value;
    assert_eq!(compressed_account.leaf_index, 0);
    let counter = &compressed_account.data.as_ref().unwrap().data;
    let counter = CounterAccount::deserialize(&mut &counter[..]).unwrap();
    let prev_counter_data = counter.data();
    assert_eq!(counter.value, 0);

    // Check that the owner of the counter is the creating program
    assert_eq!(compressed_account.owner, counter::ID);

    // Increment the counter.
    delegate_counter(&mut rpc, &payer, &compressed_account)
        .await
        .unwrap();

    // Check that the owner was changed.
    let compressed_account = rpc
        .get_compressed_account(address, None)
        .await
        .unwrap()
        .value;
    // let compressed_account = rpc
    //     .get_compressed_accounts_by_owner(&delegation::ID, None, None)
    //     .await
    //     .unwrap()
    //     .value;
    // let compressed_account = compressed_account.items.first().unwrap();
    // Check that the owner of the counter is the creating program
    assert_eq!(compressed_account.owner, delegation::ID);
    println!("compressed_account {:?}", compressed_account);

    // Check that the data of the counter is the same.
    let counter = &compressed_account.data.as_ref().unwrap().data;
    let counter = CounterAccount::deserialize(&mut &counter[..]).unwrap();
    assert_eq!(prev_counter_data, counter.data())
}

async fn create_counter<R>(
    rpc: &mut R,
    payer: &Keypair,
    address: &[u8; 32],
    address_tree_info: TreeInfo,
) -> Result<Signature, RpcError>
where
    R: Rpc + Indexer,
{
    let mut remaining_accounts = PackedAccounts::default();
    let config = SystemAccountMetaConfig::new(counter::ID);
    remaining_accounts.add_system_accounts(config);

    let rpc_result = rpc
        .get_validity_proof(
            vec![],
            vec![AddressWithTree {
                tree: address_tree_info.tree,
                address: *address,
            }],
            None,
        )
        .await?
        .value;
    let output_state_tree_index = rpc
        .get_random_state_tree_info()?
        .pack_output_tree_index(&mut remaining_accounts)?;
    let packed_address_tree_info = rpc_result
        .pack_tree_infos(&mut remaining_accounts)
        .address_trees[0];

    let instruction_data = counter::instruction::CreateCounter {
        proof: rpc_result.proof,
        address_tree_info: packed_address_tree_info,
        output_state_tree_index,
    };

    let accounts = counter::accounts::GenericAnchorAccounts {
        signer: payer.pubkey(),
        delegation_program: delegation::ID,
        delegation_cpi_signer: Pubkey::new_from_array(delegation::LIGHT_CPI_SIGNER.cpi_signer),
        noop: Pubkey::new_from_array(NOOP_PROGRAM_ID),
    };

    let (remaining_accounts_metas, _, _) = remaining_accounts.to_account_metas();

    let instruction = Instruction {
        program_id: counter::ID,
        accounts: [
            accounts.to_account_metas(Some(true)),
            remaining_accounts_metas,
        ]
        .concat(),
        data: instruction_data.data(),
    };

    rpc.create_and_send_transaction(&[instruction], &payer.pubkey(), &[payer])
        .await
}

#[allow(clippy::too_many_arguments)]
async fn delegate_counter<R>(
    rpc: &mut R,
    payer: &Keypair,
    compressed_account: &CompressedAccount,
) -> Result<Signature, RpcError>
where
    R: Rpc + Indexer,
{
    let hash = compressed_account.hash;

    let address = light_sdk::address::v2::derive_address_from_seed(&AddressSeed(delegation::LIGHT_CPI_SIGNER.cpi_signer), &rpc.get_address_tree_v2().tree, &delegation::ID);
    msg!("tree calling tree: {:?}", &rpc.get_address_tree_v2().tree);

    println!("test calling address: {:?}", address);

    let rpc_result = rpc
        //.get_validity_proof(vec![hash], vec![AddressWithTree{address, tree: rpc.get_address_tree_v2().tree}], None)
        .get_validity_proof(vec![hash], vec![], None)
        .await?
        .value;

    let mut remaining_accounts = PackedAccounts::default();
    let packed_tree_accounts = rpc_result.pack_tree_infos(&mut remaining_accounts);

    let rpc_result_creation = rpc
        .get_validity_proof(
            vec![],
            vec![AddressWithTree {
                address,
                tree: rpc.get_address_tree_v2().tree,
            }],
            None,
        )
        .await?
        .value;
    let packed_tree_creation_accounts = rpc_result_creation.pack_tree_infos(&mut remaining_accounts);
    let address_tree_info = packed_tree_creation_accounts.address_trees[0];

    let mut config = SystemAccountMetaConfig::new(counter::ID);
    config.cpi_context = rpc_result.accounts[0].tree_info.cpi_context;
    remaining_accounts.add_system_accounts_small(config).unwrap();

    let counter_account =
        CounterAccount::deserialize(&mut compressed_account.data.as_ref().unwrap().data.as_slice())
            .unwrap();

    let packed_state_tree = packed_tree_accounts.state_trees.unwrap();

    let account_meta = CompressedAccountMeta {
        tree_info: packed_state_tree.packed_tree_infos[0],
        address: compressed_account.address.unwrap(),
        output_state_tree_index: packed_state_tree.output_tree_index,
    };

    let instruction_data = counter::instruction::DelegateCounter {
        proof: rpc_result.proof,
        counter_value: counter_account.value,
        account_meta,
        address_tree_info
    };

    let accounts = counter::accounts::GenericAnchorAccounts {
        signer: payer.pubkey(),
        delegation_program: delegation::ID,
        delegation_cpi_signer: Pubkey::new_from_array(delegation::LIGHT_CPI_SIGNER.cpi_signer),
        noop: Pubkey::new_from_array(NOOP_PROGRAM_ID),
    };

    let (remaining_accounts_metas, _, _) = remaining_accounts.to_account_metas();

    let instruction = Instruction {
        program_id: counter::ID,
        accounts: [
            accounts.to_account_metas(Some(true)),
            remaining_accounts_metas,
        ]
        .concat(),
        data: instruction_data.data(),
    };

    rpc.create_and_send_transaction(&[instruction], &payer.pubkey(), &[payer])
        .await
}

#[allow(clippy::too_many_arguments)]
async fn decrement_counter<R>(
    rpc: &mut R,
    payer: &Keypair,
    compressed_account: &CompressedAccount,
) -> Result<Signature, RpcError>
where
    R: Rpc + Indexer,
{
    let mut remaining_accounts = PackedAccounts::default();
    let config = SystemAccountMetaConfig::new(counter::ID);
    remaining_accounts.add_system_accounts(config);

    let hash = compressed_account.hash;

    let rpc_result = rpc
        .get_validity_proof(Vec::from(&[hash]), vec![], None)
        .await?
        .value;

    let packed_tree_accounts = rpc_result
        .pack_tree_infos(&mut remaining_accounts)
        .state_trees
        .unwrap();

    let counter_account =
        CounterAccount::deserialize(&mut compressed_account.data.as_ref().unwrap().data.as_slice())
            .unwrap();

    let account_meta = CompressedAccountMeta {
        tree_info: packed_tree_accounts.packed_tree_infos[0],
        address: compressed_account.address.unwrap(),
        output_state_tree_index: packed_tree_accounts.output_tree_index,
    };

    let instruction_data = counter::instruction::DecrementCounter {
        proof: rpc_result.proof,
        counter_value: counter_account.value,
        account_meta,
    };

    let accounts = counter::accounts::GenericAnchorAccounts {
        signer: payer.pubkey(),
        delegation_program: delegation::ID,
        delegation_cpi_signer: Pubkey::new_from_array(delegation::LIGHT_CPI_SIGNER.cpi_signer),
        noop: Pubkey::new_from_array(NOOP_PROGRAM_ID),
    };

    let (remaining_accounts_metas, _, _) = remaining_accounts.to_account_metas();

    let instruction = Instruction {
        program_id: counter::ID,
        accounts: [
            accounts.to_account_metas(Some(true)),
            remaining_accounts_metas,
        ]
        .concat(),
        data: instruction_data.data(),
    };

    rpc.create_and_send_transaction(&[instruction], &payer.pubkey(), &[payer])
        .await
}

async fn reset_counter<R>(
    rpc: &mut R,
    payer: &Keypair,
    compressed_account: &CompressedAccount,
) -> Result<Signature, RpcError>
where
    R: Rpc + Indexer,
{
    let mut remaining_accounts = PackedAccounts::default();
    let config = SystemAccountMetaConfig::new(counter::ID);
    remaining_accounts.add_system_accounts(config);

    let hash = compressed_account.hash;

    let rpc_result = rpc
        .get_validity_proof(Vec::from(&[hash]), vec![], None)
        .await?
        .value;

    let packed_merkle_context = rpc_result
        .pack_tree_infos(&mut remaining_accounts)
        .state_trees
        .unwrap();

    let counter_account =
        CounterAccount::deserialize(&mut compressed_account.data.as_ref().unwrap().data.as_slice())
            .unwrap();

    let account_meta = CompressedAccountMeta {
        tree_info: packed_merkle_context.packed_tree_infos[0],
        address: compressed_account.address.unwrap(),
        output_state_tree_index: packed_merkle_context.output_tree_index,
    };

    let instruction_data = counter::instruction::ResetCounter {
        proof: rpc_result.proof,
        counter_value: counter_account.value,
        account_meta,
    };

    let accounts = counter::accounts::GenericAnchorAccounts {
        signer: payer.pubkey(),
        delegation_program: delegation::ID,
        delegation_cpi_signer: Pubkey::new_from_array(delegation::LIGHT_CPI_SIGNER.cpi_signer),
        noop: Pubkey::new_from_array(NOOP_PROGRAM_ID),
    };

    let (remaining_accounts_metas, _, _) = remaining_accounts.to_account_metas();

    let instruction = Instruction {
        program_id: counter::ID,
        accounts: [
            accounts.to_account_metas(Some(true)),
            remaining_accounts_metas,
        ]
        .concat(),
        data: instruction_data.data(),
    };

    rpc.create_and_send_transaction(&[instruction], &payer.pubkey(), &[payer])
        .await
}

async fn close_counter<R>(
    rpc: &mut R,
    payer: &Keypair,
    compressed_account: &CompressedAccount,
) -> Result<Signature, RpcError>
where
    R: Rpc + Indexer,
{
    let mut remaining_accounts = PackedAccounts::default();
    let config = SystemAccountMetaConfig::new(counter::ID);
    remaining_accounts.add_system_accounts(config);

    let hash = compressed_account.hash;

    let rpc_result = rpc
        .get_validity_proof(Vec::from(&[hash]), vec![], None)
        .await
        .unwrap()
        .value;

    let packed_tree_infos = rpc_result
        .pack_tree_infos(&mut remaining_accounts)
        .state_trees
        .unwrap();

    let counter_account =
        CounterAccount::deserialize(&mut compressed_account.data.as_ref().unwrap().data.as_slice())
            .unwrap();

    let account_meta = CompressedAccountMetaClose {
        tree_info: packed_tree_infos.packed_tree_infos[0],
        address: compressed_account.address.unwrap(),
    };

    let instruction_data = counter::instruction::CloseCounter {
        proof: rpc_result.proof,
        counter_value: counter_account.value,
        account_meta,
    };

    let accounts = counter::accounts::GenericAnchorAccounts {
        signer: payer.pubkey(),
        delegation_program: delegation::ID,
        delegation_cpi_signer: Pubkey::new_from_array(delegation::LIGHT_CPI_SIGNER.cpi_signer),
        noop: Pubkey::new_from_array(NOOP_PROGRAM_ID),
    };

    let (remaining_accounts_metas, _, _) = remaining_accounts.to_account_metas();

    let instruction = Instruction {
        program_id: counter::ID,
        accounts: [
            accounts.to_account_metas(Some(true)),
            remaining_accounts_metas,
        ]
        .concat(),
        data: instruction_data.data(),
    };

    rpc.create_and_send_transaction(&[instruction], &payer.pubkey(), &[payer])
        .await
}
