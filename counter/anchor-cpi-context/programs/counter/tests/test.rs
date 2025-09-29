// #![cfg(feature = "test-sbf")]

use anchor_lang::{AnchorDeserialize, Event, InstructionData, ToAccountMetas};
use counter::CounterAccount;
use delegation::CDelegationRecord;
use light_client::indexer::{CompressedAccount, TreeInfo};
use light_hasher::hash_to_field_size::hashv_to_bn254_field_size_be_const_array;
use light_program_test::{
    program_test::LightProgramTest, AddressWithTree, Indexer, ProgramTestConfig, Rpc, RpcError,
};
use light_sdk::instruction::{
    account_meta::CompressedAccountMeta, PackedAccounts, SystemAccountMetaConfig,
};
use light_sdk_types::address::AddressSeed;
use solana_pubkey::Pubkey;
use solana_sdk::{
    instruction::Instruction,
    signature::{Keypair, Signature, Signer},
};

#[tokio::test]
async fn test_counter_delegation() {
    let mut config = ProgramTestConfig::new_v2(
        true,
        Some(vec![
            ("counter", counter::ID),
            ("delegation", delegation::ID),
        ]),
    );
    config.log_light_protocol_events = true;
    let mut rpc = LightProgramTest::new(config).await.unwrap();
    rpc.payer = Keypair::from_bytes(&[74, 183, 167, 69, 156, 246, 176, 34, 96, 21, 56, 158, 76, 206, 19, 168, 27, 169, 9, 27, 92, 253, 151, 219, 95, 122, 150, 205, 24, 76, 163, 8, 6, 206, 225, 221, 60, 225, 130, 197, 220, 248, 45, 86, 98, 158, 224, 232, 103, 72, 206, 106, 36, 28, 6, 92, 195, 194, 180, 62, 127, 218, 223, 105]).unwrap();
    let payer = rpc.get_payer().insecure_clone();
    rpc.context
        .airdrop(&payer.pubkey(), 100_000_000_000_000)
        .expect("Payer airdrop failed.");

    let address_tree_info = rpc.get_address_tree_v2();

    let seed = hashv_to_bn254_field_size_be_const_array::<3>(&[
        b"counter".as_slice(),
        payer.pubkey().as_ref(),
    ])
    .unwrap();

    let address = light_sdk::address::v2::derive_address_from_seed(
        &AddressSeed(seed),
        &address_tree_info.tree,
        &counter::ID,
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

    // Delegate the counter.
    delegate_counter(&mut rpc, &payer, &compressed_account)
        .await
        .unwrap();

    // Check that the owner was changed.
    let compressed_account = rpc
        .get_compressed_account(address, None)
        .await
        .unwrap()
        .value;

    // Check that the owner of the counter is the delegation program
    assert_eq!(compressed_account.owner, delegation::ID);
    println!("\nCompressed_account {:?}", compressed_account);

    // Check that the data of the counter is the same.
    let counter = &compressed_account.data.as_ref().unwrap().data;
    let counter = CounterAccount::deserialize(&mut &counter[..]).unwrap();
    assert_eq!(prev_counter_data, counter.data());

    // Get the new counter account, deriving the address from mappedPDA
    let counter_pda =
        Pubkey::find_program_address(&[b"counter", payer.pubkey().as_ref()], &counter::ID).0;
    let cda_address = light_sdk::address::v2::derive_address_from_seed(
        &AddressSeed(counter_pda.to_bytes()),
        &rpc.get_address_tree_v2().tree,
        &delegation::ID,
    );
    let new_compressed_account = rpc
        .get_compressed_account(cda_address, None)
        .await
        .unwrap()
        .value;
    let compressed_account_data = &new_compressed_account.data.as_ref().unwrap().data;

    // Confirm that the CDelegationRecord has expected data and the counter data is unchanged
    let compressed_account =
        CDelegationRecord::deserialize(&mut &compressed_account_data[..]).unwrap();
    let counter = CounterAccount::deserialize(&mut &compressed_account.data[..]).unwrap();
    assert_eq!(prev_counter_data, counter.data());
    assert_eq!(compressed_account.pda, counter_pda);
    assert_eq!(compressed_account.address, address);
    assert_eq!(compressed_account.owner, counter::ID);
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
    remaining_accounts.add_system_accounts_small(config)?;

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
        counter_pda_account: Pubkey::find_program_address(
            &[b"counter", payer.pubkey().as_ref()],
            &counter::ID,
        )
        .0,
        delegation_program: delegation::ID,
        delegation_cpi_signer: Pubkey::new_from_array(delegation::LIGHT_CPI_SIGNER.cpi_signer),
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

    let counter_pda =
        Pubkey::find_program_address(&[b"counter", payer.pubkey().as_ref()], &counter::ID).0;

    let address = light_sdk::address::v2::derive_address_from_seed(
        &AddressSeed(counter_pda.to_bytes()),
        &rpc.get_address_tree_v2().tree,
        &delegation::ID,
    );

    let rpc_result = rpc
        .get_validity_proof(
            vec![hash],
            vec![AddressWithTree {
                address,
                tree: rpc.get_address_tree_v2().tree,
            }],
            None,
        )
        .await?
        .value;

    let mut remaining_accounts = PackedAccounts::default();
    let packed_tree_accounts = rpc_result.pack_tree_infos(&mut remaining_accounts);

    let packed_tree_creation_accounts = rpc_result.pack_tree_infos(&mut remaining_accounts);
    let address_tree_info = packed_tree_creation_accounts.address_trees[0];

    let mut config = SystemAccountMetaConfig::new(counter::ID);
    config.cpi_context = rpc_result.accounts[0].tree_info.cpi_context;
    remaining_accounts
        .add_system_accounts_small(config)
        .unwrap();

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
        address_tree_info,
    };

    let accounts = counter::accounts::GenericAnchorAccounts {
        signer: payer.pubkey(),
        counter_pda_account: counter_pda,
        delegation_program: delegation::ID,
        delegation_cpi_signer: Pubkey::new_from_array(delegation::LIGHT_CPI_SIGNER.cpi_signer),
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
