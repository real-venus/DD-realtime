use anchor_lang::AnchorDeserialize;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone, Default)]
pub struct ObMarketInfo {
    pub name: String,
    pub address: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub bids: Pubkey,
    pub asks: Pubkey,
    pub event_queue: Pubkey,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub base_lot_size: u64,
    pub quote_lot_size: u64,
}
impl ObMarketInfo {
    pub fn is_valid_account(&self, account: &Pubkey) -> bool {
        account.eq(&self.bids) || account.eq(&self.asks) || account.eq(&self.event_queue)
    }
}

#[derive(Copy, Clone, AnchorDeserialize)]
#[cfg_attr(target_endian = "little", derive(Debug))]
#[repr(packed)]
pub struct ObMarketState {
    // 0
    pub account_flags: u64, // Initialized, Market

    // 1
    pub own_address: [u64; 4],

    // 5
    pub vault_signer_nonce: u64,
    // 6
    pub coin_mint: [u64; 4],
    // 10
    pub pc_mint: [u64; 4],

    // 14
    pub coin_vault: [u64; 4],
    // 18
    pub coin_deposits_total: u64,
    // 19
    pub coin_fees_accrued: u64,

    // 20
    pub pc_vault: [u64; 4],
    // 24
    pub pc_deposits_total: u64,
    // 25
    pub pc_fees_accrued: u64,

    // 26
    pub pc_dust_threshold: u64,

    // 27
    pub req_q: [u64; 4],
    // 31
    pub event_q: [u64; 4],

    // 35
    pub bids: [u64; 4],
    // 39
    pub asks: [u64; 4],

    // 43
    pub coin_lot_size: u64,
    // 44
    pub pc_lot_size: u64,

    // 45
    pub fee_rate_bps: u64,
    // 46
    pub referrer_rebates_accrued: u64,
}
