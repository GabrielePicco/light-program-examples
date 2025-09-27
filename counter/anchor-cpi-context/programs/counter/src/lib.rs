#![allow(unexpected_cfgs)]

use anchor_lang::{prelude::*, AnchorDeserialize, Discriminator};
use light_sdk::{
    account::LightAccount,
    cpi::{CpiAccounts, CpiInputs, CpiSigner},
    derive_light_cpi_signer,
    instruction::{
        account_meta::{CompressedAccountMeta},
        PackedAddressTreeInfo, ValidityProof,
    },
    LightDiscriminator, LightHasher,
};
use light_batched_merkle_tree::queue::BatchedQueueAccount;
use light_compressed_account::{
    address::derive_address,
    instruction_data::{
        with_readonly::{InstructionDataInvokeCpiWithReadOnly},
    },
};
use light_hasher::{
    hash_to_field_size::hashv_to_bn254_field_size_be_const_array,
};
use light_sdk::cpi::{CpiAccountsSmall, InvokeLightSystemProgram};
use light_sdk_types::{
    cpi_context_write::CpiContextWriteAccounts, CpiAccountsConfig,
};

declare_id!("H3WD4CZ5GFxxJtqC8vNqHRPfepfRXGNVZeAFdEat9cgv");

pub const LIGHT_CPI_SIGNER: CpiSigner =
    derive_light_cpi_signer!("H3WD4CZ5GFxxJtqC8vNqHRPfepfRXGNVZeAFdEat9cgv");

#[program]
pub mod counter {
    use light_compressed_account::instruction_data::data::ReadOnlyAddress;
    use super::*;

    pub fn create_counter<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: ValidityProof,
        address_tree_info: PackedAddressTreeInfo,
        output_state_tree_index: u8,
    ) -> Result<()> {
        let cpi_accounts = CpiAccounts::new(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            LIGHT_CPI_SIGNER,
        );
        let seed = hashv_to_bn254_field_size_be_const_array::<3>(&[
            b"counter".as_slice(),
            ctx.accounts.signer.key().as_ref(),
        ]).unwrap();
        let address = derive_address(
            &seed,
            &cpi_accounts.tree_pubkeys().unwrap()
                [address_tree_info.address_merkle_tree_pubkey_index as usize]
                .to_bytes(),
            &ID.to_bytes(),
        );

        let new_address_params = address_tree_info.into_new_address_params_packed(seed.into());
        let mut counter = LightAccount::<'_, CounterAccount>::new_init(
            &ID,
            Some(address),
            output_state_tree_index,
        );

        counter.owner = ctx.accounts.signer.key();
        counter.value = 0;

        let cpi = CpiInputs::new_with_address(
            proof,
            vec![counter.to_account_info().map_err(ProgramError::from)?],
            vec![new_address_params],
        );
        cpi.invoke_light_system_program(cpi_accounts)
            .map_err(ProgramError::from)?;

        Ok(())
    }

    pub fn delegate_counter<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: ValidityProof,
        counter_value: u64,
        account_meta: CompressedAccountMeta,
        address_tree_info: PackedAddressTreeInfo,
    ) -> Result<()> {
        let mut account_meta = account_meta;
        let light_cpi_accounts = CpiAccountsSmall::new_with_config(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            CpiAccountsConfig {
                cpi_context: true,
                cpi_signer: LIGHT_CPI_SIGNER,
                sol_compression_recipient: false,
                sol_pool_pda: false,
            },
        );

        // Change the owner to the delegation program and zero out the data, hash and discriminator
        let counter = LightAccount::<'_, CounterAccount>::new_mut(
            &ID,
            &account_meta,
            CounterAccount {
                owner: ctx.accounts.signer.key(),
                value: counter_value,
            },
        ).map_err(ProgramError::from)?;

        let cpi_context_accounts = CpiContextWriteAccounts {
            fee_payer: light_cpi_accounts.fee_payer(),
            authority: light_cpi_accounts.authority().unwrap(),
            cpi_context: light_cpi_accounts.cpi_context().unwrap(),
            cpi_signer: LIGHT_CPI_SIGNER,
        };
        let in_account = counter
            .to_in_account()
            .ok_or(ProgramError::InvalidAccountData)?;
        let mut out_account = counter
            .to_output_compressed_account_with_packed_context(Some(delegation::ID))
            .map_err(ProgramError::from)?
            .ok_or(ProgramError::InvalidAccountData)?;
        out_account.compressed_account.data = None;
        InstructionDataInvokeCpiWithReadOnly::new(
            LIGHT_CPI_SIGNER.program_id.into(),
            LIGHT_CPI_SIGNER.bump,
            None,
        )
            .mode_v2()
            .with_input_compressed_accounts(vec![in_account])
            .with_output_compressed_accounts(vec![out_account])
            .invoke_write_to_cpi_context_first(&cpi_context_accounts.to_account_infos())?;

        // Change the owner to the delegation program
        let account_info = light_cpi_accounts.get_tree_account_info(1).unwrap();
        let output_queue = BatchedQueueAccount::output_from_account_info(account_info).unwrap();
        account_meta.tree_info.leaf_index = output_queue.batch_metadata.next_index as u32;
        account_meta.tree_info.prove_by_index = true;
        let counter = LightAccount::<'_, CounterAccount>::new_mut(
            &delegation::ID,
            &account_meta,
            CounterAccount {
                owner: ctx.accounts.signer.key(),
                value: counter_value,
            },
        ).map_err(ProgramError::from)?;
        let mut in_account = counter
            .to_in_account()
            .ok_or(ProgramError::InvalidAccountData)?;
        in_account.discriminator = [0u8; 8];
        in_account.data_hash = [0u8; 32];
        let out_account = counter
            .to_output_compressed_account_with_packed_context(None)
            .map_err(ProgramError::from)?
            .unwrap();

        // CPI into the delegation program
        {
            let cpi_accounts = delegation::cpi::accounts::Delegate {
                signer: ctx.accounts.signer.to_account_info(),
                pda: ctx.accounts.counter_pda_account.to_account_info(),
                delegation_program: ctx.accounts.delegation_program.to_account_info(),
                delegation_cpi_signer: ctx.accounts.delegation_cpi_signer.to_account_info(),
                light_system_program: ctx.remaining_accounts[0].to_account_info(),
            };
            let cpi_program = ctx.accounts.delegation_program.to_account_info();
            let signer = ctx.accounts.signer.key;
            let pda_seeds: &[&[&[u8]]] = &[&[
                b"counter",
                signer.as_ref(),
                &[ctx.bumps.counter_pda_account],
            ]];
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, pda_seeds);
            let cpi_ctx = cpi_ctx.with_remaining_accounts(light_cpi_accounts.to_account_infos().into());
            delegation::cpi::delegate(
                cpi_ctx,
                proof,
                account_meta,
                in_account,
                out_account,
                address_tree_info,
            )?;
        }
        Ok(())
    }

}

#[error_code]
pub enum CustomError {
    #[msg("No authority to perform this action")]
    Unauthorized,
    #[msg("Counter overflow")]
    Overflow,
    #[msg("Counter underflow")]
    Underflow,
}

#[derive(Accounts)]
pub struct GenericAnchorAccounts<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(mut, seeds = [b"counter", signer.key().as_ref()], bump)]
    pub counter_pda_account: AccountInfo<'info>,
    pub delegation_program: Program<'info, delegation::program::Delegation>,
    #[account(address = Pubkey::new_from_array(delegation::LIGHT_CPI_SIGNER.cpi_signer))]
    pub delegation_cpi_signer: AccountInfo<'info>,
}

// declared as event so that it is part of the idl.
#[event]
#[derive(Clone, Debug, Default, LightDiscriminator, LightHasher)]
pub struct CounterAccount {
    #[hash]
    pub owner: Pubkey,
    pub value: u64,
}
