#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use light_sdk::{
    account::LightAccount,
    cpi::{CpiAccounts, CpiInputs, CpiSigner},
    derive_light_cpi_signer,
    instruction::{
        account_meta::{CompressedAccountMeta, CompressedAccountMetaClose},
        PackedAddressTreeInfo, ValidityProof,
    },
    LightDiscriminator, LightHasher,
};
use light_batched_merkle_tree::queue::BatchedQueueAccount;
use light_compressed_account::instruction_data::cpi_context::CompressedCpiContext;
use light_compressed_account::instruction_data::with_account_info::CompressedAccountInfo;
use light_sdk_types::CpiAccountsConfig;
use light_compressed_account::instruction_data::data::OutputCompressedAccountWithPackedContext;
use light_compressed_account::instruction_data::with_readonly::InAccount;
use light_sdk::cpi::{create_light_system_progam_instruction_invoke_cpi, invoke_light_system_program};
use light_compressed_account::instruction_data::with_readonly::InstructionDataInvokeCpiWithReadOnly;
use light_sdk::cpi::InvokeLightSystemProgram;

declare_id!("DELeGr1ZdNJ6PK8zu9g4Zvw1H85Wgy3Up5Eh7uo9XDHZ");

pub const LIGHT_CPI_SIGNER: CpiSigner =
    derive_light_cpi_signer!("DELeGr1ZdNJ6PK8zu9g4Zvw1H85Wgy3Up5Eh7uo9XDHZ");

#[program]
pub mod delegation {
    use light_compressed_account::instruction_data::data::{NewAddressParams, NewAddressParamsAssignedPacked};
    use light_sdk::cpi::{CpiAccountsSmall, WithLightAccount};
    use light_sdk_types::address::AddressSeed;
    use super::*;

    pub fn delegate<'info>(
        ctx: Context<'_, '_, '_, 'info, Delegate<'info>>,
        proof: ValidityProof,
        account_meta: CompressedAccountMeta,
        in_account: InAccount,
        out_account: OutputCompressedAccountWithPackedContext,
        address_tree_info: PackedAddressTreeInfo,
    ) -> Result<()> {
        let mut out_account = out_account;

        let light_cpi_accounts = CpiAccountsSmall::new_with_config(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            CpiAccountsConfig {
                cpi_context: true,
                cpi_signer: crate::LIGHT_CPI_SIGNER,
                sol_compression_recipient: false,
                sol_pool_pda: false,
            },
        );

        // Derive the address and set it
        // let tree_account_pk = pubkey!("amt2kaJA14v3urZbZvnc5v2np8jqvc4Z8zDep5wbtzx");
        // let seed = AddressSeed(ctx.accounts.delegation_cpi_signer.key.to_bytes());
        // let address = light_sdk::address::v2::derive_address_from_seed(&seed, &tree_account_pk, &ID);
        // msg!("test receiving address: {:?}", address);
        // msg!("tree receiving account: {:?}", tree_account_info.key);
        //out_account.compressed_account.address = Some(address);

        msg!("Cpi signer: {}", Pubkey::new_from_array(LIGHT_CPI_SIGNER.cpi_signer));
        msg!("Cpi signer received: {}", ctx.accounts.delegation_cpi_signer.to_account_info().key);
        let mut light_cpi_accounts = light_cpi_accounts.to_account_infos().to_vec();
        light_cpi_accounts[1] = ctx.accounts.delegation_cpi_signer.to_account_info();
        msg!("Light cpi accounts: {:?}", light_cpi_accounts.to_account_infos().iter().map(|ai| ai.key).collect::<Vec<_>>());

        let tree_account_pk = pubkey!("amt2kaJA14v3urZbZvnc5v2np8jqvc4Z8zDep5wbtzx");
        let seed = AddressSeed(ctx.accounts.delegation_cpi_signer.key.to_bytes());
        let address = light_sdk::address::v2::derive_address_from_seed(&seed, &tree_account_pk, &ID);
        let new_address_params = address_tree_info.into_new_address_params_packed(seed);

        let mut counter = LightAccount::<'_, CounterAccount>::new_init(
            &ID,
            Some(address),
            account_meta.output_state_tree_index,
        );
        counter.value = 100;

        InstructionDataInvokeCpiWithReadOnly::new(
            LIGHT_CPI_SIGNER.program_id.into(),
            LIGHT_CPI_SIGNER.bump,
            proof.into(),
        )
            .mode_v2()
            .with_input_compressed_accounts(vec![in_account])
            .with_output_compressed_accounts(vec![out_account])
            .with_new_address_params(vec![NewAddressParamsAssignedPacked::new(new_address_params, Some(2))])
            .with_light_account(counter).unwrap()
            .invoke_execute_cpi_context(light_cpi_accounts.as_slice())?;

        // Create the new account, setting the expected address and set it
        // let tree_account_pk = pubkey!("amt2kaJA14v3urZbZvnc5v2np8jqvc4Z8zDep5wbtzx");
        // let seed = AddressSeed(ctx.accounts.delegation_cpi_signer.key.to_bytes());
        // let address = light_sdk::address::v2::derive_address_from_seed(&seed, &tree_account_pk, &ID);
        // msg!("test receiving address: {:?}", address);
        //
        // let cpi_accounts = CpiAccounts::new(
        //     ctx.accounts.signer.as_ref(),
        //     ctx.remaining_accounts,
        //     crate::LIGHT_CPI_SIGNER,
        // );
        // let new_address_params = address_tree_info.into_new_address_params_packed(seed);
        // let mut counter = LightAccount::<'_, CounterAccount>::new_init(
        //     &ID,
        //     Some(address),
        //     account_meta.output_state_tree_index,
        // );
        // counter.owner = ctx.accounts.signer.key();
        // counter.value = 0;
        // let cpi = CpiInputs::new_with_address(
        //     proof,
        //     vec![counter.to_account_info().map_err(ProgramError::from)?],
        //     vec![new_address_params],
        // );
        // let bump = cpi_accounts.bump();
        // let mut account_infos = cpi_accounts.to_account_infos();
        // account_infos.push(ctx.accounts.noop_program.clone());
        // account_infos.push(ctx.accounts.delegation_cpi_signer.clone());
        // let instruction = create_light_system_progam_instruction_invoke_cpi(cpi, cpi_accounts).map_err(ProgramError::from)?;
        // invoke_light_system_program(account_infos.as_slice(), instruction, bump).map_err(ProgramError::from)?;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Delegate<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    /// CHECK: delegation program
    pub delegation_program: AccountInfo<'info>,
    /// CHECK: cpi signer
    pub delegation_cpi_signer: AccountInfo<'info>,
    /// CHECK: caller cpi signer
    // pub caller_cpi_signer: AccountInfo<'info>,
    /// CHECK: light system program
    pub light_system_program: AccountInfo<'info>,
    /// CHECK: noop program
    pub noop_program: AccountInfo<'info>,
}

#[event]
#[derive(Clone, Debug, Default, LightDiscriminator, LightHasher)]
pub struct CounterAccount {
    #[hash]
    pub owner: Pubkey,
    pub value: u64,
}

