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

declare_id!("DELeGr1ZdNJ6PK8zu9g4Zvw1H85Wgy3Up5Eh7uo9XDHZ");

pub const LIGHT_CPI_SIGNER: CpiSigner =
    derive_light_cpi_signer!("DELeGr1ZdNJ6PK8zu9g4Zvw1H85Wgy3Up5Eh7uo9XDHZ");

#[program]
pub mod delegation {
    use light_batched_merkle_tree::queue::BatchedQueueAccount;
    use light_compressed_account::instruction_data::cpi_context::CompressedCpiContext;
    use light_compressed_account::instruction_data::with_account_info::CompressedAccountInfo;
    use light_sdk_types::CpiAccountsConfig;
    use super::*;

    pub fn delegate<'info>(
        ctx: Context<'_, '_, '_, 'info, Delegate<'info>>,
        proof: ValidityProof,
        account_meta: CompressedAccountMeta,
        data: Vec<u8>,
        compressed_account: CompressedAccountInfo
    ) -> Result<()> {
        let mut account_meta = account_meta;
        let light_cpi_accounts = CpiAccounts::new_with_config(
            ctx.accounts.signer.as_ref(),
            ctx.remaining_accounts,
            CpiAccountsConfig {
                cpi_context: true,
                cpi_signer: crate::LIGHT_CPI_SIGNER,
                sol_compression_recipient: false,
                sol_pool_pda: false,
            },
        );
        msg!("Greetings from: {:?}", ctx.program_id);
        msg!(
            "tree pubkeys {:?} ",
            light_cpi_accounts.tree_pubkeys().unwrap()
        );
        let account_info = light_cpi_accounts.get_tree_account_info(1).unwrap();

        let output_queue = BatchedQueueAccount::output_from_account_info(account_info).unwrap();
        account_meta.tree_info.leaf_index = output_queue.batch_metadata.next_index as u32;
        account_meta.tree_info.prove_by_index = true;
        let pk = anchor_lang::prelude::Pubkey::default();
        let counter = LightAccount::<'_, CounterAccount>::new_init(
            &pk,
            None,
            account_meta.output_state_tree_index,
        );
        //
        let cpi_inputs = CpiInputs {
            proof,
            account_infos: Some(vec![counter
                .to_account_info()
                .map_err(ProgramError::from)?]),
            new_assigned_addresses: None,
            cpi_context: Some(CompressedCpiContext {
                set_context: false,
                first_set_context: false,
                cpi_context_account_index: 0,
            }),
            ..Default::default()
        };
        cpi_inputs
            .invoke_light_system_program(light_cpi_accounts)
            .map_err(ProgramError::from)?;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Delegate<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
}
