#![allow(unexpected_cfgs)]

use anchor_lang::{prelude::*, AnchorDeserialize, Discriminator};
use light_compressed_account::instruction_data::data::NewAddressParamsAssignedPacked;
use light_compressed_account::instruction_data::with_readonly::InstructionDataInvokeCpiWithReadOnly;
use light_hasher::hash_to_field_size::hashv_to_bn254_field_size_be_const_array;
use light_sdk::cpi::WithLightAccount;
use light_sdk::cpi::{CpiAccountsSmall, InvokeLightSystemProgram};
use light_sdk::{
    account::LightAccount,
    cpi::CpiSigner,
    derive_light_cpi_signer,
    instruction::{account_meta::CompressedAccountMeta, PackedAddressTreeInfo, ValidityProof},
    LightDiscriminator, LightHasher,
};
use light_sdk_types::address::AddressSeed;
use light_sdk_types::{cpi_context_write::CpiContextWriteAccounts, CpiAccountsConfig};

declare_id!("H3WD4CZ5GFxxJtqC8vNqHRPfepfRXGNVZeAFdEat9cgv");

pub const LIGHT_CPI_SIGNER: CpiSigner =
    derive_light_cpi_signer!("H3WD4CZ5GFxxJtqC8vNqHRPfepfRXGNVZeAFdEat9cgv");

#[program]
pub mod counter {
    use super::*;

    pub fn create_counter<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: ValidityProof,
        address_tree_info: PackedAddressTreeInfo,
        output_state_tree_index: u8,
    ) -> Result<()> {
        let cpi_accounts = CpiAccountsSmall::new(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            LIGHT_CPI_SIGNER,
        );
        msg!("CPI Accounts: {:?}", cpi_accounts.to_account_infos().iter().map(|a| a.key.to_string()).collect::<Vec<String>>());
        msg!("Remaining accounts: {:?}", ctx.remaining_accounts.iter().map(|a| a.key.to_string()).collect::<Vec<String>>());
        let seed = hashv_to_bn254_field_size_be_const_array::<3>(&[
            b"counter".as_slice(),
            ctx.accounts.signer.key().as_ref(),
        ])
        .map_err(|_| ProgramError::InvalidSeeds)?;
        msg!("Output state tree index: {}", output_state_tree_index);
        msg!("Address tree info: {:?}", address_tree_info);

        let address = light_sdk::address::v2::derive_address_from_seed(
            &AddressSeed(seed),
            &cpi_accounts.tree_pubkeys().unwrap()
                [address_tree_info.address_merkle_tree_pubkey_index as usize],
            &ID,
        );

        msg!("Seed: {:?}", seed);
        msg!("Tree: {:?}", cpi_accounts.tree_pubkeys().unwrap()[address_tree_info.address_merkle_tree_pubkey_index as usize]);
        msg!("Program: {:?}", ID);
        msg!("Address: {:?}", Pubkey::new_from_array(address).to_string());

        let mut counter = LightAccount::<'_, CounterAccount>::new_init(
            &ID,
            Some(address),
            output_state_tree_index,
        );
        counter.owner = ctx.accounts.signer.key();
        counter.value = 0;

        InstructionDataInvokeCpiWithReadOnly::new(
            LIGHT_CPI_SIGNER.program_id.into(),
            LIGHT_CPI_SIGNER.bump,
            proof.into(),
        )
        .mode_v2()
        .with_light_account(counter)
        .map_err(ProgramError::from)?
        .with_new_address_params(vec![NewAddressParamsAssignedPacked::new(
            address_tree_info.into_new_address_params_packed(seed.into()),
            Some(0),
        )])
        .invoke(cpi_accounts.to_account_infos().as_slice())?;

        Ok(())
    }

    pub fn increment_counter<'info>(
        ctx: Context<'_, '_, '_, 'info, IncrementCounter<'info>>,
    ) -> Result<()> {
        let counter_account_info = &ctx.accounts.counter_pda_account;
        let mut counter_data = CounterAccount::try_from_slice(&counter_account_info.data.borrow())?;
        if counter_data.owner != ctx.accounts.signer.key() {
            return Err(CustomError::Unauthorized.into());
        }
        counter_data.value = counter_data.value.checked_add(1).ok_or(CustomError::Overflow)?;
        counter_data.serialize(&mut &mut counter_account_info.data.borrow_mut()[..])?;
        Ok(())
    }

    pub fn increment_compressed_counter<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: ValidityProof,
        counter_value: u64,
        account_meta: CompressedAccountMeta,
    ) -> Result<()> {

        let cpi_accounts = CpiAccountsSmall::new(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            LIGHT_CPI_SIGNER,
        );

        let mut counter = LightAccount::<'_, CounterAccount>::new_mut(
            &ID,
            &account_meta,
            CounterAccount {
                owner: ctx.accounts.signer.key(),
                value: counter_value,
            },
        ).map_err(ProgramError::from)?;

        counter.value = counter.value.checked_add(1).ok_or(CustomError::Overflow)?;

        InstructionDataInvokeCpiWithReadOnly::new(
            LIGHT_CPI_SIGNER.program_id.into(),
            LIGHT_CPI_SIGNER.bump,
            proof.into(),
        )
            .mode_v2()
            .with_light_account(counter)
            .map_err(ProgramError::from)?
            .invoke(cpi_accounts.to_account_infos().as_slice())?;
        Ok(())
    }


    pub fn delegate_counter<'info>(
        ctx: Context<'_, '_, '_, 'info, GenericAnchorAccounts<'info>>,
        proof: ValidityProof,
        counter_value: u64,
        account_meta: CompressedAccountMeta,
        address_tree_info: PackedAddressTreeInfo,
    ) -> Result<()> {
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
        )
        .map_err(ProgramError::from)?;
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
        .with_input_compressed_accounts(vec![in_account.clone()])
        .with_output_compressed_accounts(vec![out_account])
        .invoke_write_to_cpi_context_first(&cpi_context_accounts.to_account_infos())?;

        // CPI into the delegation program
        {
            let out_account = counter
                .to_output_compressed_account_with_packed_context(Some(delegation::ID))
                .map_err(ProgramError::from)?
                .ok_or(ProgramError::InvalidAccountData)?;

            let cpi_accounts = delegation::cpi::accounts::Delegate {
                signer: ctx.accounts.signer.to_account_info(),
                pda: ctx.accounts.counter_pda_account.to_account_info(),
                delegation_program: ctx.accounts.delegation_program.to_account_info(),
                delegation_cpi_signer: ctx.accounts.delegation_cpi_signer.to_account_info(),
                light_system_program: ctx.remaining_accounts[0].to_account_info(),
            };
            let cpi_program = ctx.accounts.delegation_program.to_account_info();
            let signer = ctx.accounts.signer.key;
            let bump = ctx.bumps.counter_pda_account;
            let signer_seeds_no_bump: Vec<Vec<u8>> =
                vec![b"counter".to_vec(), signer.to_bytes().to_vec()];
            let signers_seeds = signer_seeds_no_bump.clone();
            let signer_seeds: Vec<&[u8]> = signers_seeds.iter().map(|s| s.as_slice()).collect();
            let signer_seeds: &[&[&[u8]]] = &[&[signer_seeds[0], signer_seeds[1], &[bump]]];

            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);
            let cpi_ctx = cpi_ctx.with_remaining_accounts(light_cpi_accounts.to_account_infos());
            delegation::cpi::delegate(
                cpi_ctx,
                proof,
                account_meta,
                in_account,
                out_account,
                address_tree_info,
                ID,
                signer_seeds_no_bump,
                bump,
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
    /// CHECK: This is the counter PDA account
    #[account(mut, seeds = [b"counter", signer.key().as_ref()], bump)]
    pub counter_pda_account: AccountInfo<'info>,
    pub delegation_program: Program<'info, delegation::program::Delegation>,
    /// CHECK: This is the CPI signer derived from the delegation program
    #[account(address = Pubkey::new_from_array(delegation::LIGHT_CPI_SIGNER.cpi_signer))]
    pub delegation_cpi_signer: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct IncrementCounter<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    /// CHECK: This is the counter PDA account
    #[account(mut, seeds = [b"counter", signer.key().as_ref()], bump)]
    pub counter_pda_account: AccountInfo<'info>,
}

// declared as event so that it is part of the idl.
#[event]
#[derive(Clone, Debug, Default, LightDiscriminator, LightHasher)]
pub struct CounterAccount {
    #[hash]
    pub owner: Pubkey,
    pub value: u64,
}
