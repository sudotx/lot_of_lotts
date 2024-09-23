use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("CLuTuUqHsE2arQGuTrVN1oYr74mZG1johNFQvmZTj6tW");

#[program]
pub mod lottery_system {
    use super::*;

    const ONE_WEEK:i64 = 604800; // 604800 seconds in a week

    /// Initializes a new creator pool with the specified pool ID and total tickets.
    ///
    /// This function creates a new creator pool with the given pool ID and total tickets. The total tickets must be between 1 and 10,000 (inclusive).
    ///
    /// # Arguments
    /// * `ctx` - The context for the transaction, containing the necessary accounts.
    /// * `pool_id` - The unique identifier for the creator pool.
    /// * `total_tickets` - The total number of tickets available in the creator pool.
    ///
    /// # Errors
    /// This function may return the following errors:
    /// - `LotteryError::InvalidTotalTickets` - If the total tickets is less than 1 or greater than 10,000.
    /// - `ProgramError` - If there is an error accessing the necessary accounts or updating the creator pool data.
    pub fn initialize_creator_pool(
        ctx: Context<InitializeCreatorPool>,
        total_tickets: u64,
    ) -> Result<()> {
        require!(total_tickets > 0, LotteryError::InvalidTotalTickets);

        let creator_pool = &mut ctx.accounts.creator_pool;

        let state = &mut ctx.accounts.state;
        let pool_id = state.next_pool_id;
        state.next_pool_id += 1;

        creator_pool.pool_id = pool_id;
        creator_pool.authority = ctx.accounts.authority.key();
        creator_pool.last_draw_time = Clock::get()?.unix_timestamp;
        creator_pool.total_tickets = total_tickets;
        creator_pool.is_active = true;

        // add the new pool ID to the active pools list
        state.active_pools.push(pool_id);

        Ok(())
    }

    pub fn accumulate_tickets(ctx: Context<BuyTickets>) -> Result<()> {
        let user_trading_volume = ctx.accounts.user_trading_stats.volume;

        let dollar_value_in_lamports: u64 = 20_000_000_000_000; // $2 in lamports

        // Calculate base tickets based on trading volume
        // For example, 1 ticket per 100 units of trading volume
        let base_tickets = user_trading_volume / dollar_value_in_lamports;

        // Calculate bonus tickets
        let bonus_tickets = user_trading_volume / 100;

        let total_tickets = base_tickets + bonus_tickets;

        // Use integer arithmetic for NFT bonus
        let nft_bonus = 100 + (20 * ctx.accounts.user_nfts.count.min(5));

        let final_tickets = (total_tickets * nft_bonus as u64) / 100;

        // Update user tickets
        let user_tickets = &mut ctx.accounts.user_tickets;
        user_tickets.balance += final_tickets;
        user_tickets.last_purchase_time = Clock::get()?.unix_timestamp;

        // Update creator pool
        let creator_pool = &mut ctx.accounts.creator_pool;
        creator_pool.total_tickets += total_tickets;

        msg!("Accumulated {} tickets for user", total_tickets);
        Ok(())
    }

    /// Draws the lottery and distributes the prize to the winner.
    ///
    /// This function is responsible for the following:
    /// - Checking if the lottery can be drawn (last draw was at least 4 hours ago)
    /// - Generating a random winning ticket number
    /// - Finding the winner based on the total number of tickets
    /// - Checking if the winner is a JEET (purchased tickets within the last 10 minutes)
    /// - Calculating the prize amount based on the total number of tickets and the ticket price
    /// - Transferring the prize amount from the vault to the winner's token account
    /// - Resetting the creator pool for the next lottery draw
    ///
    /// # Errors
    /// This function may return the following errors:
    /// - `LotteryError::DrawTooEarly`: If the last lottery draw was less than 4 hours ago
    /// - `LotteryError::NoWinner`: If no winner could be found
    /// - `LotteryError::WinnerNotFound`: If the winner's account could not be found
    pub fn draw_lottery(ctx: Context<DrawLottery>) -> Result<()> {
        let creator_pool = &mut ctx.accounts.creator_pool;
        let current_time = Clock::get()?.unix_timestamp;

        // lottery is drawn every 4 hours
        require!(
            current_time - creator_pool.last_draw_time >= 4 * 60 * 60,
            LotteryError::DrawTooEarly
        );

        let state = &mut ctx.accounts.state;

        // checks if the pool id is in the active pools list
        require!(
            state.active_pools.contains(&creator_pool.pool_id),
            LotteryError::PoolNotActive
        );

        let seed = current_time as u64;
        let winning_ticket = seed % creator_pool.total_tickets;

        // Optimize winner selection
        let winner = ctx
            .remaining_accounts
            .iter()
            .try_fold((0, None), |(acc, winner), account| {
                let user_tickets = Account::<UserTickets>::try_from(account)?;
                let new_acc = acc + user_tickets.balance;
                Ok((
                    new_acc,
                    if new_acc > winning_ticket && winner.is_none() {
                        Some(user_tickets.key())
                    } else {
                        winner
                    },
                ))
            })
            .map_err(|_: ProgramError| LotteryError::NoWinner)?
            .1
            .ok_or(LotteryError::NoWinner)?;

        // Check for JEET penalty
        let winner_account = ctx
            .remaining_accounts
            .iter()
            .find(|a| a.key() == winner)
            .ok_or(LotteryError::WinnerNotFound)?;
        let winner_tickets = Account::<UserTickets>::try_from(winner_account)?;
        let is_jeet = current_time - winner_tickets.last_purchase_time < 10 * 60;

        // fecth price from oracle
        // let feed_account = ctx.accounts.feed.solana_oracle.borrow();
        // let feed = PullFeedAccountData::parse(feed_account).unwrap();
        // let price = feed.value();

        let price: u64 = 2;
        let prize_amount = creator_pool.total_tickets * price; // $2 per ticket

        let prize_to_distribute = if is_jeet {
            prize_amount / 2
        } else {
            prize_amount
        };

        // transfer tokens from vault to winner
        let transfer_instruction = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.winner_token_account.to_account_info(),
            authority: ctx.accounts.vault_token_account.to_account_info(),
        };
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                transfer_instruction,
            ),
            prize_to_distribute,
        )?;

        // Burn tickets after lottery is drawn
        for account in ctx.remaining_accounts.iter() {
            let mut user_tickets = Account::<UserTickets>::try_from(account)?;
            user_tickets.balance = 0;
        }

        // Reset for next draw
        creator_pool.last_draw_time = current_time;
        creator_pool.total_tickets = 0;

        // Update creator pool
        creator_pool.last_draw_time = Clock::get()?.unix_timestamp;

        msg!("Drawn lottery for user");
        msg!("Total tickets: {}", creator_pool.total_tickets);
        msg!("Last draw time: {}", creator_pool.last_draw_time);
        msg!("Prize amount: {}", prize_to_distribute);
        msg!("Winner: {}", winner);

        Ok(())
    }

    /// Voids expired tickets and resets the ticket balance and last purchase time.
    ///
    /// # Arguments
    /// * `ctx` - The context for the void expired tickets operation, containing the user's ticket account.
    ///
    /// # Errors
    /// * None
    ///
    /// # Notes
    /// - This function sets the `is_void` flag on the user's ticket to `true` if the ticket was purchased more than a week ago.
    /// - If the ticket is void, the function resets the ticket balance to 0 and updates the last purchase time to the current time.
    pub fn void_expired_tickets(ctx: Context<VoidExpiredTickets>) -> Result<()> {
        let user_tickets = &mut ctx.accounts.user_tickets;
        let current_time = Clock::get()?.unix_timestamp;
        let week_ago = current_time - ONE_WEEK; // 604800 seconds in a week

         // Check if tickets are expired
        if user_tickets.last_purchase_time >= week_ago {
            msg!("Tickets are not expired for user: {}", user_tickets.user);
            return Ok(()); // Early return if tickets are not expired
        }

        // Set the isVoid flag on the ticket to true only if expired
        if !user_tickets.is_void {
            user_tickets.is_void = true;
            user_tickets.balance = 0; // Reset balance
            user_tickets.last_purchase_time = current_time; // Update last purchase time
            msg!("Voided tickets for user: {}", user_tickets.user);
        } else {
            msg!("Tickets already voided for user: {}", user_tickets.user);
        }

        Ok(())
    }

    /// Withdraws fees from the creator pool.
    ///
    /// # Arguments
    /// * `ctx` - The context for the withdraw fees operation, containing the creator pool and authority accounts.
    /// * `amount` - The amount of fees to withdraw.
    ///
    /// # Errors
    /// * `LotteryError::InvalidAuthority` - If the authority account does not match the creator pool's authority.
    /// * `LotteryError::InsufficientBalance` - If the creator pool's total fees are less than the requested withdrawal amount.
    ///
    /// # Notes
    /// - This function checks that the authority account matches the creator pool's authority.
    /// - It then subtracts the requested amount from the creator pool's total fees and sets the pool's `is_active` flag to `false`.
    /// - Finally, it transfers the requested amount from the vault token account to the authority account.
    pub fn withdraw_fees(ctx: Context<WithdrawFees>, amount: u64) -> Result<()> {
        let authority = &ctx.accounts.authority;
        require_eq!(
            authority.key,
            &ctx.accounts.creator_pool.authority,
            LotteryError::InvalidAuthority
        );

        require!(amount > 0, LotteryError::InvalidAmount); // Ensure amount is positive
        // ensure pool is active
        require!(ctx.accounts.creator_pool.is_active, LotteryError::PoolNotActive);


        let vault_balance = ctx.accounts.vault_token_account.amount;
        require!(vault_balance >= amount, LotteryError::InsufficientBalance);

        let transfer_instruction = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.authority.to_account_info(),
            authority: ctx.accounts.vault_token_account.to_account_info(),
        };
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                transfer_instruction,
            ),
            amount,
        )
        .map_err(|_| LotteryError::TransferFailed)?;


        msg!("Withdrawn {} from the creator pool by {}", amount, authority.key());

        Ok(())
    }

    /// Gets the current balance of tickets for the user.
    ///
    /// # Arguments
    /// * `ctx` - The context for the get user ticket balance operation, containing the user's ticket account.
    ///
    /// # Returns
    /// The current balance of tickets for the user.
    pub fn get_user_ticket_balance(ctx: Context<GetUserTicketBalance>) -> Result<u64> {
        // add function to swap token for tickets
        Ok(ctx.accounts.user_tickets.balance)
    }

    /// Gets the current status of the creator pool.
    ///
    /// # Arguments
    /// * `ctx` - The context for the get creator pool status operation, containing the creator pool account.
    ///
    /// # Returns
    /// `true` if the creator pool is active, `false` otherwise.
    pub fn get_creator_pool_status(ctx: Context<GetCreatorPoolStatus>) -> Result<bool> {
        Ok(ctx.accounts.creator_pool.is_active)
    }
}

#[derive(Accounts)]
pub struct InitializeCreatorPool<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 8 + 8 + 8 + 1 + 8 // discriminator + pubkey + last_draw_time + total_tickets + total_fees + is_active + pool_id
    )]
    pub creator_pool: Account<'info, CreatorPool>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub state: Account<'info, State>,
}

#[derive(Accounts)]
pub struct BuyTickets<'info> {
    #[account(mut)]
    pub creator_pool: Account<'info, CreatorPool>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    #[account(mut)]
    pub buyer_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(
        init_if_needed,
        payer = buyer,
        space = 8 + 32 + 8 + 8 // discriminator + pubkey + balance + timestamp
    )]
    pub user_tickets: Account<'info, UserTickets>,
    pub user_nfts: Account<'info, UserNFTs>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub user_trading_stats: Account<'info, UserTradingStats>,
}

#[derive(Accounts)]
pub struct DrawLottery<'info> {
    #[account(mut)]
    pub creator_pool: Account<'info, CreatorPool>,
    #[account(mut)]
    pub winner: Signer<'info>,
    #[account(mut)]
    pub winner_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub fee_wallet: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub state: Account<'info, State>,
}

#[derive(Accounts)]
pub struct SellTickets<'info> {
    #[account(mut)]
    pub user_tickets: Account<'info, UserTickets>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}
#[derive(Accounts)]
pub struct VoidExpiredTickets<'info> {
    #[account(mut)]
    pub user_tickets: Account<'info, UserTickets>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct DistributePenalty<'info> {
    #[account(mut)]
    pub user_tickets: Account<'info, UserTickets>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct GetUserTicketBalance<'info> {
    #[account(mut)]
    pub user_tickets: Account<'info, UserTickets>,
}

#[derive(Accounts)]
pub struct GetCreatorPoolStatus<'info> {
    #[account(mut)]
    pub creator_pool: Account<'info, CreatorPool>,
}

#[derive(Accounts)]
pub struct WithdrawFees<'info> {
    #[account(mut)]
    pub creator_pool: Account<'info, CreatorPool>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct CreatorPool {
    pub authority: Pubkey,
    pub last_draw_time: i64,
    pub total_tickets: u64,
    pub is_active: bool,
    pub pool_id: u64,
}

#[account]
pub struct UserTickets {
    pub user: Pubkey,
    pub balance: u64,
    pub last_purchase_time: i64,
    pub is_void: bool,
}

#[account]
pub struct UserNFTs {
    pub user: Pubkey,
    pub count: u8,
}

#[account]
pub struct State {
    pub authority: Pubkey,
    pub next_pool_id: u64,
    pub active_pools: Vec<u64>,
}

#[account]
pub struct UserTradingStats {
    pub user: Pubkey,
    pub volume: u64,
    pub total_tickets_bought: u64,
    pub total_tickets_sold: u64,
    pub total_tickets_voided: u64,
}

#[error_code]
pub enum LotteryError {
    #[msg("It's too early for the next draw")]
    DrawTooEarly,
    #[msg("No winner found")]
    NoWinner,
    #[msg("Winner account not found")]
    WinnerNotFound,
    #[msg("Invalid Authority")]
    InvalidAuthority,
    #[msg("Insufficient Balance")]
    InsufficientBalance,
    #[msg("Invalid Total Tickets")]
    InvalidTotalTickets,
    #[msg("Pool Already Exists")]
    PoolExists,
    #[msg("Ticket is Void")]
    VoidTicket,
    #[msg("Invalid Amount")]
    InvalidAmount,
    #[msg("Transfer Failed")]
    TransferFailed,
    #[msg("Pool Not Active")]
    PoolNotActive,
}
