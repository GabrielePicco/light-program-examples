#![allow(unexpected_cfgs)]

use anchor_lang::prelude::borsh::{BorshDeserialize, BorshSerialize};
use anchor_lang::prelude::*;
use light_compressed_account::instruction_data::data::NewAddressParamsAssignedPacked;
use light_compressed_account::instruction_data::data::OutputCompressedAccountWithPackedContext;
use light_compressed_account::instruction_data::with_readonly::InAccount;
use light_compressed_account::instruction_data::with_readonly::InstructionDataInvokeCpiWithReadOnly;
use light_sdk::cpi::InvokeLightSystemProgram;
use light_sdk::cpi::{CpiAccountsSmall, WithLightAccount};
use light_sdk::{
    account::LightAccount,
    cpi::CpiSigner,
    derive_light_cpi_signer,
    instruction::{PackedAddressTreeInfo, ValidityProof},
    LightDiscriminator, LightHasher,
};
use light_sdk_types::address::AddressSeed;
use light_sdk_types::CpiAccountsConfig;

declare_id!("DELeGr1ZdNJ6PK8zu9g4Zvw1H85Wgy3Up5Eh7uo9XDHZ");

pub const LIGHT_CPI_SIGNER: CpiSigner =
    derive_light_cpi_signer!("DELeGr1ZdNJ6PK8zu9g4Zvw1H85Wgy3Up5Eh7uo9XDHZ");

pub const TREE_ACCOUNT_PUBKEY: Pubkey = pubkey!("amt2kaJA14v3urZbZvnc5v2np8jqvc4Z8zDep5wbtzx");

#[program]
pub mod delegation {
    use super::*;

    pub fn delegate<'info>(
        ctx: Context<'_, '_, '_, 'info, Delegate<'info>>,
        proof: ValidityProof,
        in_account: InAccount,
        out_account: OutputCompressedAccountWithPackedContext,
        address_tree_info: PackedAddressTreeInfo,
        output_state_tree_index: u8,
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

        let mut light_cpi_accounts = light_cpi_accounts.to_account_infos().to_vec();
        light_cpi_accounts[1] = ctx.accounts.delegation_cpi_signer.to_account_info();

        let seed = AddressSeed(ctx.accounts.pda.key.to_bytes());
        let address =
            light_sdk::address::v2::derive_address_from_seed(&seed, &TREE_ACCOUNT_PUBKEY, &ID);
        let new_address_params = address_tree_info.into_new_address_params_packed(seed);

        let mut compressed_data = LightAccount::<'_, CDelegationRecord>::new_init(
            &ID,
            Some(address),
            output_state_tree_index,
        );
        compressed_data.data = out_account
            .compressed_account
            .data
            .clone()
            .map(|d| d.data)
            .unwrap_or_default();
        compressed_data.pda = ctx.accounts.pda.key();
        compressed_data.lamports = 0;
        compressed_data.delegation_slot = Clock::get()?.slot;
        compressed_data.address = in_account.address.unwrap_or_default();

        InstructionDataInvokeCpiWithReadOnly::new(
            LIGHT_CPI_SIGNER.program_id.into(),
            LIGHT_CPI_SIGNER.bump,
            proof.into(),
        )
        .mode_v2()
        .with_input_compressed_accounts(vec![in_account])
        .with_output_compressed_accounts(vec![out_account])
        .with_new_address_params(vec![NewAddressParamsAssignedPacked::new(
            new_address_params,
            Some(2),
        )])
        .with_light_account(compressed_data)
        .map_err(ProgramError::from)?
        .invoke_execute_cpi_context(light_cpi_accounts.as_slice())?;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Delegate<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    pub pda: Signer<'info>,
    /// CHECK: delegation program
    pub delegation_program: AccountInfo<'info>,
    /// CHECK: cpi signer
    pub delegation_cpi_signer: AccountInfo<'info>,
    /// CHECK: light system program
    pub light_system_program: AccountInfo<'info>,
}

#[derive(
    Clone, Debug, Default, LightDiscriminator, BorshDeserialize, BorshSerialize, LightHasher,
)]
pub struct CDelegationRecord {
    #[hash]
    pub address: [u8; 32],
    #[hash]
    pub pda: Pubkey,
    #[hash]
    pub authority: Pubkey,
    #[hash]
    pub owner: Pubkey,
    pub delegation_slot: u64,
    pub lamports: u64,
    #[hash]
    pub data: Vec<u8>,
}
