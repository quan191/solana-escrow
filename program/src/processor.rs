use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    sysvar::{rent::Rent, Sysvar},
};

use spl_token::state::Account as TokenAccount;

use crate::{error::EscrowError, instruction::EscrowInstruction, state::Escrow};

pub struct Processor;
impl Processor {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = EscrowInstruction::unpack(instruction_data)?;

        match instruction {
            EscrowInstruction::InitEscrow { amount } => {
                msg!("Instruction: InitEscrow");
                Self::process_init_escrow(accounts, amount, program_id)
            }
            EscrowInstruction::Deposit { amount } => {
                msg!("Instruction: Deposit");
                Self::process_deposit(accounts, amount, program_id)
            }
            EscrowInstruction::Withdraw { amount } => {
                msg!("Instruction: Withdraw");
                Self::process_withdraw(accounts, amount, program_id)
            }
        }
    }

    fn process_init_escrow(
        accounts: &[AccountInfo],
        amount: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        // an iterator loop through all account slice 
        let account_info_iter = &mut accounts.iter();
        // first account is the one who sign for this tx
        let initializer = next_account_info(account_info_iter)?;

        if !initializer.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }
        // next is the account x for only purpose . that is hold token x of account A 
        let temp_token_account = next_account_info(account_info_iter)?;
        // next is the account for hold token Y of user A of account token X ( owner is program id of token y )
        let token_to_receive_account = next_account_info(account_info_iter)?;
        if *token_to_receive_account.owner != spl_token::id() {
            return Err(ProgramError::IncorrectProgramId);
        }
        // account user rent for hold token 
        let escrow_account = next_account_info(account_info_iter)?;
        let rent = &Rent::from_account_info(next_account_info(account_info_iter)?)?;

        if !rent.is_exempt(escrow_account.lamports(), escrow_account.data_len()) {
            return Err(EscrowError::NotRentExempt.into());
        }
        // check if this escrow account has been initialized or not 
        let mut escrow_info = Escrow::unpack_unchecked(&escrow_account.try_borrow_data()?)?;
        if escrow_info.is_initialized() {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        escrow_info.is_initialized = true;
        escrow_info.initializer_pubkey = *initializer.key;
        escrow_info.temp_token_account_pubkey = *temp_token_account.key;
        escrow_info.initializer_token_to_receive_account_pubkey = *token_to_receive_account.key;
        escrow_info.expected_amount = amount;

        Escrow::pack(escrow_info, &mut escrow_account.try_borrow_mut_data()?)?;
        // load pda ???
        let (pda, _nonce) = Pubkey::find_program_address(&[b"escrow"], program_id);

        // get the token program 
        let token_program = next_account_info(account_info_iter)?;
        let owner_change_ix = spl_token::instruction::set_authority(
            token_program.key,
            temp_token_account.key,
            Some(&pda),
            spl_token::instruction::AuthorityType::AccountOwner,
            initializer.key,
            &[&initializer.key],
        )?;

        msg!("Calling the token program to transfer token account ownership...");
        invoke(
            &owner_change_ix,
            &[
                temp_token_account.clone(),
                initializer.clone(),
                token_program.clone(),
            ],
        )?;

        Ok(())
    }

    fn process_deposit(
        accounts: &[AccountInfo],
        amount_deposit: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        // taker is bob 
        let taker = next_account_info(account_info_iter)?;

        if !taker.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // create token x account to send to esceow , is the sending account
        let takers_sending_token_account = next_account_info(account_info_iter)?;
        
        // create token X account to rêcive from escrow 
        // let takers_token_to_receive_account = next_account_info(account_info_iter)?;

        // the account create for hold token Y of pda 
        let pdas_temp_token_account = next_account_info(account_info_iter)?;
        let pdas_temp_token_account_info =
            TokenAccount::unpack(&pdas_temp_token_account.try_borrow_data()?)?;
        let (pda, nonce) = Pubkey::find_program_address(&[b"escrow"], program_id);
        
        // // the amount Y want to exchange 
        // if amount_expected_by_taker != pdas_temp_token_account_info.amount {
        //     return Err(EscrowError::ExpectedAmountMismatch.into());
        // }

        // get account of A lice 
        let initializers_main_account = next_account_info(account_info_iter)?;
        // let initializers_token_to_receive_account = next_account_info(account_info_iter)?;
        let escrow_account = next_account_info(account_info_iter)?;

        let escrow_info = Escrow::unpack(&escrow_account.try_borrow_data()?)?;

        if escrow_info.temp_token_account_pubkey != *pdas_temp_token_account.key {
            return Err(ProgramError::InvalidAccountData);
        }

        if escrow_info.initializer_pubkey != *initializers_main_account.key {
            return Err(ProgramError::InvalidAccountData);
        }

        // if escrow_info.initializer_token_to_receive_account_pubkey
        //     != *initializers_token_to_receive_account.key
        // {
        //     return Err(ProgramError::InvalidAccountData);
        // }
        // get the token program  ( token Y )
        let token_program = next_account_info(account_info_iter)?;

        // Bob now transfer token X from taker_sending_token_account to pdas X account 
        let transfer_to_initializer_ix = spl_token::instruction::transfer(
            token_program.key,
            takers_sending_token_account.key,
            pdas_temp_token_account.key,
            taker.key,
            &[&taker.key],
            amount_deposit,
        )?;
        msg!("Calling the token program to transfer tokens to the escrow's initializer...");
        invoke(
            &transfer_to_initializer_ix,
            &[
                takers_sending_token_account.clone(),
                pdas_temp_token_account.clone(),
                taker.clone(),
                token_program.clone(),
            ],
        )?;
        // pda 
        // let pda_account = next_account_info(account_info_iter)?;
        // // transfer token X from pdas temp token account to takers_token_to_receive_account of Bob 
        // let transfer_to_taker_ix = spl_token::instruction::transfer(
        //     token_program.key,
        //     pdas_temp_token_account.key,
        //     takers_token_to_receive_account.key,
        //     &pda,
        //     &[&pda],
        //     pdas_temp_token_account_info.amount,
        // )?;
        // msg!("Calling the token program to transfer tokens to the taker...");
        // invoke_signed(
        //     &transfer_to_taker_ix,
        //     &[
        //         pdas_temp_token_account.clone(),
        //         takers_token_to_receive_account.clone(),
        //         pda_account.clone(),
        //         token_program.clone(),
        //     ],
        //     &[&[&b"escrow"[..], &[nonce]]],
        // )?;
        // // close temp token account 
        // let close_pdas_temp_acc_ix = spl_token::instruction::close_account(
        //     token_program.key,
        //     pdas_temp_token_account.key,
        //     initializers_main_account.key,
        //     &pda,
        //     &[&pda],
        // )?;
        // msg!("Calling the token program to close pda's temp account...");
        // invoke_signed(
        //     &close_pdas_temp_acc_ix,
        //     &[
        //         pdas_temp_token_account.clone(),
        //         initializers_main_account.clone(),
        //         pda_account.clone(),
        //         token_program.clone(),
        //     ],
        //     &[&[&b"escrow"[..], &[nonce]]],
        // )?;

        // msg!("Closing the escrow account...");
        // **initializers_main_account.try_borrow_mut_lamports()? = initializers_main_account
        //     .lamports()
        //     .checked_add(escrow_account.lamports())
        //     .ok_or(EscrowError::AmountOverflow)?;
        // **escrow_account.try_borrow_mut_lamports()? = 0;
        // *escrow_account.try_borrow_mut_data()? = &mut [];

        Ok(())
    }

    fn process_withdraw(accounts: &[AccountInfo],
        amount_withdraw: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pdas_temp_token_account = next_account_info(account_info_iter)?;
        let pdas_temp_token_account_info =
            TokenAccount::unpack(&pdas_temp_token_account.try_borrow_data()?)?;
        let (pda, nonce) = Pubkey::find_program_address(&[b"escrow"], program_id);
        let initializers_main_account = next_account_info(account_info_iter)?;
        let initializers_token_to_receive_account = next_account_info(account_info_iter)?;
        let escrow_account = next_account_info(account_info_iter)?;

        let escrow_info = Escrow::unpack(&escrow_account.try_borrow_data()?)?;

        if escrow_info.temp_token_account_pubkey != *pdas_temp_token_account.key {
            return Err(ProgramError::InvalidAccountData);
        }

        if escrow_info.initializer_pubkey != *initializers_main_account.key {
            return Err(ProgramError::InvalidAccountData);
        }

        let token_program = next_account_info(account_info_iter)?;
        let pda_account = next_account_info(account_info_iter)?;
        let transfer_to_initializer_ix = spl_token::instruction::transfer(
            token_program.key,
            pdas_temp_token_account.key,
            initializers_token_to_receive_account.key,
            &pda,
            &[&pda],
            pdas_temp_token_account_info.amount,
        )?;
        msg!("Calling the token program to transfer tokens to the signer...");
        invoke_signed(
            &transfer_to_initializer_ix,
            &[
                pdas_temp_token_account.clone(),
                initializers_token_to_receive_account.clone(),
                pda_account.clone(),
                token_program.clone(),
            ],
            &[&[&b"escrow"[..], &[nonce]]],
        )?;

        let close_pdas_temp_acc_ix = spl_token::instruction::close_account(
            token_program.key,
            pdas_temp_token_account.key,
            initializers_main_account.key,
            &pda,
            &[&pda],
        )?;
        msg!("Calling the token program to close pda's temp account...");
        invoke_signed(
            &close_pdas_temp_acc_ix,
            &[
                pdas_temp_token_account.clone(),
                initializers_main_account.clone(),
                pda_account.clone(),
                token_program.clone(),
            ],
            &[&[&b"escrow"[..], &[nonce]]],
        )?;

        msg!("Closing the escrow account...");
        **initializers_main_account.try_borrow_mut_lamports()? = initializers_main_account
            .lamports()
            .checked_add(escrow_account.lamports())
            .ok_or(EscrowError::AmountOverflow)?;
        **escrow_account.try_borrow_mut_lamports()? = 0;
        *escrow_account.try_borrow_mut_data()? = &mut [];

        Ok(())
    }
}
