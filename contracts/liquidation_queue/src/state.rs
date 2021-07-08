use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_bignumber::{Decimal256, Uint256};
use cosmwasm_std::{CanonicalAddr, Order, ReadonlyStorage, StdError, StdResult, Storage, Uint128};
use cosmwasm_storage::{singleton, singleton_read, Bucket, ReadonlyBucket};
use std::convert::TryInto;

static KEY_CONFIG: &[u8] = b"config";
static KEY_BID_IDX: &[u8] = b"bid_idx";

static PREFIX_BID: &[u8] = b"bid";
static PREFIX_BID_BY_USER: &[u8] = b"bid_by_user";
static PREFIX_BID_POOL_BY_COLLATERAL: &[u8] = b"bid_pool_by_col";
static PREFIX_TOTAL_BIDS_BY_COLLATERAL: &[u8] = b"total_bids_by_col";
static PREFIX_COLLATERAL_INFO: &[u8] = b"col_info";
static PREFIX_EPOCH_SCALE_SUM: &[u8] = b"epoch_scale_sum";

const MAX_LIMIT: u8 = 30;
const DEFAULT_LIMIT: u8 = 10;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: CanonicalAddr,
    pub oracle_contract: CanonicalAddr,
    pub stable_denom: String,
    pub safe_ratio: Decimal256,
    pub bid_fee: Decimal256,
    pub liquidation_threshold: Uint256,
    pub price_timeframe: u64,
    pub waiting_period: u64,
}

pub fn store_config<S: Storage>(storage: &mut S, config: &Config) -> StdResult<()> {
    singleton(storage, KEY_CONFIG).save(config)
}

pub fn read_config<S: ReadonlyStorage>(storage: &S) -> StdResult<Config> {
    singleton_read(storage, KEY_CONFIG).load()
}

pub fn pop_bid_idx<S: Storage>(storage: &mut S) -> StdResult<Uint128> {
    let mut idx_store = singleton(storage, KEY_BID_IDX);
    let last_idx: Uint128 = idx_store.load().unwrap_or(Uint128::from(1u128));
    idx_store.save(&(last_idx + Uint128::from(1u128)))?;
    Ok(last_idx)
}

pub fn store_total_bids<S: Storage>(
    storage: &mut S,
    collateral_token: &CanonicalAddr,
    total_bids: Uint256,
) -> StdResult<()> {
    let mut total_bids_bucket: Bucket<S, Uint256> =
        Bucket::new(PREFIX_TOTAL_BIDS_BY_COLLATERAL, storage);
    total_bids_bucket.save(&collateral_token.as_slice(), &total_bids)?;

    Ok(())
}

pub fn read_total_bids<S: Storage>(
    storage: &S,
    collateral_token: &CanonicalAddr,
) -> StdResult<Uint256> {
    let total_bids_bucket: ReadonlyBucket<S, Uint256> =
        ReadonlyBucket::new(PREFIX_TOTAL_BIDS_BY_COLLATERAL, storage);
    total_bids_bucket.load(&collateral_token.as_slice())
}

pub fn store_epoch_scale_sum<S: Storage>(
    storage: &mut S,
    collateral_token: &CanonicalAddr,
    premium_slot: u8,
    epoch: Uint128,
    scale: Uint128,
    sum: Decimal256,
) -> StdResult<()> {
    let mut epoch_scale_sum: Bucket<S, Decimal256> = Bucket::multilevel(
        &[
            PREFIX_EPOCH_SCALE_SUM,
            collateral_token.as_slice(),
            &premium_slot.to_be_bytes(),
            &epoch.u128().to_be_bytes(),
        ],
        storage,
    );
    epoch_scale_sum.save(&scale.u128().to_be_bytes(), &sum)
}

pub fn read_epoch_scale_sum<S: Storage>(
    storage: &S,
    collateral_token: &CanonicalAddr,
    premium_slot: u8,
    epoch: Uint128,
    scale: Uint128,
) -> StdResult<Decimal256> {
    let epoch_scale_sum: ReadonlyBucket<S, Decimal256> = ReadonlyBucket::multilevel(
        &[
            PREFIX_EPOCH_SCALE_SUM,
            collateral_token.as_slice(),
            &premium_slot.to_be_bytes(),
            &epoch.u128().to_be_bytes(),
        ],
        storage,
    );
    epoch_scale_sum.load(&scale.u128().to_be_bytes())
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CollateralInfo {
    pub collateral_token: CanonicalAddr,
    pub bid_threshold: Uint256,
    pub max_slot: u8,
}

pub fn store_collateral_info<S: Storage>(
    storage: &mut S,
    collateral_token: &CanonicalAddr,
    collateral_info: &CollateralInfo,
) -> StdResult<()> {
    let mut collateral_info_bucket: Bucket<S, CollateralInfo> =
        Bucket::new(PREFIX_COLLATERAL_INFO, storage);
    collateral_info_bucket.save(&collateral_token.as_slice(), &collateral_info)?;

    Ok(())
}

pub fn read_collateral_info<S: Storage>(
    storage: &S,
    collateral_token: &CanonicalAddr,
) -> StdResult<CollateralInfo> {
    let collateral_info_bucket: ReadonlyBucket<S, CollateralInfo> =
        ReadonlyBucket::new(PREFIX_COLLATERAL_INFO, storage);
    collateral_info_bucket
        .load(&collateral_token.as_slice())
        .map_err(|_| StdError::generic_err("Collateral is not whitelisted"))
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BidPool {
    pub sum_snapshot: Decimal256,
    pub product_snapshot: Decimal256,
    pub total_bid_amount: Uint256,
    pub premium_rate: Decimal256,
    pub current_epoch: Uint128,
    pub current_scale: Uint128,
}

pub fn store_bid_pool<S: Storage>(
    storage: &mut S,
    collateral_token: &CanonicalAddr,
    premium_slot: u8,
    bid_pool: &BidPool,
) -> StdResult<()> {
    let mut bid_pool_bucket: Bucket<S, BidPool> = Bucket::multilevel(
        &[PREFIX_BID_POOL_BY_COLLATERAL, collateral_token.as_slice()],
        storage,
    );
    bid_pool_bucket.save(&premium_slot.to_be_bytes(), &bid_pool)
}

pub fn read_bid_pool<S: Storage>(
    storage: &S,
    collateral_token: &CanonicalAddr,
    premium_slot: u8,
) -> StdResult<BidPool> {
    let bid_pool_bucket: ReadonlyBucket<S, BidPool> = ReadonlyBucket::multilevel(
        &[PREFIX_BID_POOL_BY_COLLATERAL, collateral_token.as_slice()],
        storage,
    );
    bid_pool_bucket
        .load(&premium_slot.to_be_bytes())
        .map_err(|_| StdError::generic_err("Bid pool not found"))
}

pub fn read_or_create_bid_pool<S: Storage>(
    storage: &mut S,
    collateral_info: &CollateralInfo,
    premium_slot: u8,
) -> StdResult<BidPool> {
    let bid_pool_bucket: ReadonlyBucket<S, BidPool> = ReadonlyBucket::multilevel(
        &[
            PREFIX_BID_POOL_BY_COLLATERAL,
            collateral_info.collateral_token.as_slice(),
        ],
        storage,
    );

    match bid_pool_bucket.load(&premium_slot.to_be_bytes()) {
        Ok(bid_pool) => Ok(bid_pool),
        Err(_) => {
            if (0..collateral_info.max_slot).contains(&premium_slot) {
                let bid_pool = BidPool {
                    product_snapshot: Decimal256::one(),
                    sum_snapshot: Decimal256::zero(),
                    total_bid_amount: Uint256::zero(),
                    premium_rate: Decimal256::percent(premium_slot as u64),
                    current_epoch: Uint128::zero(),
                    current_scale: Uint128::zero(),
                };
                store_bid_pool(
                    storage,
                    &collateral_info.collateral_token,
                    premium_slot,
                    &bid_pool,
                )?;
                Ok(bid_pool)
            } else {
                return Err(StdError::generic_err("Invalid premium slot"));
            }
        }
    }
}

pub fn read_bid_pools<S: Storage>(
    storage: &S,
    collateral_token: &CanonicalAddr,
    start_after: Option<u8>,
    limit: Option<u8>,
) -> StdResult<Vec<BidPool>> {
    let bid_pool_bucket: ReadonlyBucket<S, BidPool> = ReadonlyBucket::multilevel(
        &[PREFIX_BID_POOL_BY_COLLATERAL, collateral_token.as_slice()],
        storage,
    );

    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = calc_range_start(start_after);

    bid_pool_bucket
        .range(start.as_deref(), None, Order::Ascending)
        .take(limit)
        .map(|elem| {
            let (_, pool) = elem?;
            Ok(pool)
        })
        .collect()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct Bid {
    pub idx: Uint128,
    pub collateral_token: CanonicalAddr,
    pub premium_slot: u8,
    pub bidder: CanonicalAddr,
    pub amount: Uint256,
    pub product_snapshot: Decimal256,
    pub sum_snapshot: Decimal256,
    pub pending_liquidated_collateral: Uint256,
    pub wait_end: Option<u64>,
    pub epoch_snapshot: Uint128,
    pub scale_snapshot: Uint128,
}

pub fn store_bid<S: Storage>(storage: &mut S, bid_idx: Uint128, bid: &Bid) -> StdResult<()> {
    let mut bid_bucket: Bucket<S, Bid> = Bucket::new(PREFIX_BID, storage);
    bid_bucket.save(&bid_idx.u128().to_be_bytes(), &bid)?;

    let mut bid_indexer_by_user: Bucket<S, bool> = Bucket::multilevel(
        &[
            PREFIX_BID_BY_USER,
            bid.collateral_token.as_slice(),
            bid.bidder.as_slice(),
        ],
        storage,
    );
    bid_indexer_by_user.save(&bid_idx.u128().to_be_bytes(), &true)?;

    Ok(())
}

pub fn remove_bid<S: Storage>(storage: &mut S, bid_idx: Uint128) -> StdResult<()> {
    let bid: Bid = read_bid(storage, bid_idx)?;
    let mut bid_bucket: Bucket<S, Bid> = Bucket::new(PREFIX_BID, storage);
    bid_bucket.remove(&bid_idx.u128().to_be_bytes());

    // remove indexer
    let mut bid_indexer_by_user: Bucket<S, bool> = Bucket::multilevel(
        &[
            PREFIX_BID_BY_USER,
            bid.collateral_token.as_slice(),
            bid.bidder.as_slice(),
        ],
        storage,
    );
    bid_indexer_by_user.remove(&bid_idx.u128().to_be_bytes());

    Ok(())
}

pub fn read_bid<S: ReadonlyStorage>(storage: &S, bid_idx: Uint128) -> StdResult<Bid> {
    let bid_bucket: ReadonlyBucket<S, Bid> = ReadonlyBucket::new(PREFIX_BID, storage);
    bid_bucket
        .load(&bid_idx.u128().to_be_bytes())
        .map_err(|_| StdError::generic_err("No bids with the specified information exist"))
}

pub fn read_bids_by_user<S: ReadonlyStorage>(
    storage: &S,
    collateral_token: &CanonicalAddr,
    bidder: &CanonicalAddr,
    start_after: Option<Uint128>,
    limit: Option<u8>,
) -> StdResult<Vec<Bid>> {
    let bid_user_index: ReadonlyBucket<S, bool> = ReadonlyBucket::multilevel(
        &[
            PREFIX_BID_BY_USER,
            collateral_token.as_slice(),
            bidder.as_slice(),
        ],
        storage,
    );

    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = calc_range_start_idx(start_after);

    bid_user_index
        .range(start.as_deref(), None, Order::Ascending)
        .take(limit)
        .map(|elem| {
            let (k, _) = elem?;
            read_bid(storage, Uint128(bytes_to_u128(&k)?))
        })
        .collect()
}

fn bytes_to_u128(data: &[u8]) -> StdResult<u128> {
    match data[0..16].try_into() {
        Ok(bytes) => Ok(u128::from_be_bytes(bytes)),
        Err(_) => Err(StdError::generic_err(
            "Corrupted data found. 16 byte expected.",
        )),
    }
}

// this will set the first key after the provided key, by appending a 1 byte
fn calc_range_start_idx(start_after: Option<Uint128>) -> Option<Vec<u8>> {
    start_after.map(|idx| {
        let mut v = idx.u128().to_be_bytes().to_vec();
        v.push(1);
        v
    })
}

// this will set the first key after the provided key, by appending a 1 byte
fn calc_range_start(start_after: Option<u8>) -> Option<Vec<u8>> {
    start_after.map(|id| {
        let mut v = id.to_be_bytes().to_vec();
        v.push(1);
        v
    })
}
