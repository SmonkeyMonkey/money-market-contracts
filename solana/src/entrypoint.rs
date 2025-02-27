#![cfg(not(feature = "no-entrypoint"))]

use moneymarket_market::contract::handle;
use solana_program::{account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, msg, pubkey::Pubkey};

entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8]
) -> ProgramResult{
 crate::processor::process_instruction(&program_id, accounts, instruction_data)
}