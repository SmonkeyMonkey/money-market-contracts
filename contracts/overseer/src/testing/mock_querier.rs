use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_bignumber::{Decimal256, Uint256};
use cosmwasm_std::testing::{MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{
    from_binary, from_slice, to_binary, Api, Coin, Decimal, Extern, HumanAddr, Querier,
    QuerierResult, QueryRequest, SystemError, Uint128, WasmQuery,
};
use std::collections::HashMap;

use moneymarket::liquidation::LiquidationAmountResponse;
use moneymarket::market::{BorrowerInfoResponse, EpochStateResponse};
use moneymarket::oracle::PriceResponse;
use moneymarket::tokens::TokensHuman;

use terra_cosmwasm::{TaxCapResponse, TaxRateResponse, TerraQuery, TerraQueryWrapper, TerraRoute};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Query epoch state to market contract
    EpochState {
        block_height: Option<u64>,
        distributed_interest: Option<Uint256>,
    },
    /// Query loan amount to market contract
    BorrowerInfo {
        borrower: HumanAddr,
        block_height: Option<u64>,
    },
    /// Query oracle price to oracle contract
    Price { base: String, quote: String },
    /// Query liquidation amount to liquidation model contract
    LiquidationAmount {
        borrow_amount: Uint256,
        borrow_limit: Uint256,
        collaterals: TokensHuman,
        collateral_prices: Vec<Decimal256>,
    },
}

/// mock_dependencies is a drop-in replacement for cosmwasm_std::testing::mock_dependencies
/// this uses our CustomQuerier.
pub fn mock_dependencies(
    canonical_length: usize,
    contract_balance: &[Coin],
) -> Extern<MockStorage, MockApi, WasmMockQuerier> {
    let contract_addr = HumanAddr::from(MOCK_CONTRACT_ADDR);
    let custom_querier: WasmMockQuerier = WasmMockQuerier::new(
        MockQuerier::new(&[(&contract_addr, contract_balance)]),
        MockApi::new(canonical_length),
    );

    Extern {
        storage: MockStorage::default(),
        api: MockApi::new(canonical_length),
        querier: custom_querier,
    }
}

pub struct WasmMockQuerier {
    base: MockQuerier<TerraQueryWrapper>,
    tax_querier: TaxQuerier,
    epoch_state_querier: EpochStateQuerier,
    oracle_price_querier: OraclePriceQuerier,
    loan_amount_querier: LoanAmountQuerier,
    liquidation_percent_querier: LiquidationPercentQuerier,
}

#[derive(Clone, Default)]
pub struct TaxQuerier {
    rate: Decimal,
    // this lets us iterate over all pairs that match the first string
    caps: HashMap<String, Uint128>,
}

impl TaxQuerier {
    pub fn new(rate: Decimal, caps: &[(&String, &Uint128)]) -> Self {
        TaxQuerier {
            rate,
            caps: caps_to_map(caps),
        }
    }
}

pub(crate) fn caps_to_map(caps: &[(&String, &Uint128)]) -> HashMap<String, Uint128> {
    let mut owner_map: HashMap<String, Uint128> = HashMap::new();
    for (denom, cap) in caps.iter() {
        owner_map.insert(denom.to_string(), **cap);
    }
    owner_map
}

#[derive(Clone, Default)]
pub struct OraclePriceQuerier {
    // this lets us iterate over all pairs that match the first string
    oracle_price: HashMap<(String, String), (Decimal256, u64, u64)>,
}

impl OraclePriceQuerier {
    pub fn new(oracle_price: &[(&(String, String), &(Decimal256, u64, u64))]) -> Self {
        OraclePriceQuerier {
            oracle_price: oracle_price_to_map(oracle_price),
        }
    }
}

pub(crate) fn oracle_price_to_map(
    oracle_price: &[(&(String, String), &(Decimal256, u64, u64))],
) -> HashMap<(String, String), (Decimal256, u64, u64)> {
    let mut oracle_price_map: HashMap<(String, String), (Decimal256, u64, u64)> = HashMap::new();
    for (base_quote, oracle_price) in oracle_price.iter() {
        oracle_price_map.insert((*base_quote).clone(), **oracle_price);
    }

    oracle_price_map
}

#[derive(Clone, Default)]
pub struct EpochStateQuerier {
    // this lets us iterate over all pairs that match the first string
    epoch_state: HashMap<HumanAddr, (Uint256, Decimal256)>,
}

impl EpochStateQuerier {
    pub fn new(epoch_state: &[(&HumanAddr, &(Uint256, Decimal256))]) -> Self {
        EpochStateQuerier {
            epoch_state: epoch_state_to_map(epoch_state),
        }
    }
}

pub(crate) fn epoch_state_to_map(
    epoch_state: &[(&HumanAddr, &(Uint256, Decimal256))],
) -> HashMap<HumanAddr, (Uint256, Decimal256)> {
    let mut epoch_state_map: HashMap<HumanAddr, (Uint256, Decimal256)> = HashMap::new();
    for (market_contract, epoch_state) in epoch_state.iter() {
        epoch_state_map.insert((*market_contract).clone(), **epoch_state);
    }
    epoch_state_map
}

#[derive(Clone, Default)]
pub struct LoanAmountQuerier {
    // this lets us iterate over all pairs that match the first string
    borrower_amount: HashMap<HumanAddr, Uint256>,
}

impl LoanAmountQuerier {
    pub fn new(borrower_amount: &[(&HumanAddr, &Uint256)]) -> Self {
        LoanAmountQuerier {
            borrower_amount: borrower_amount_to_map(borrower_amount),
        }
    }
}

pub(crate) fn borrower_amount_to_map(
    borrower_amount: &[(&HumanAddr, &Uint256)],
) -> HashMap<HumanAddr, Uint256> {
    let mut borrower_amount_map: HashMap<HumanAddr, Uint256> = HashMap::new();
    for (market_contract, borrower_amount) in borrower_amount.iter() {
        borrower_amount_map.insert((*market_contract).clone(), **borrower_amount);
    }
    borrower_amount_map
}

impl Querier for WasmMockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        // MockQuerier doesn't support Custom, so we ignore it completely here
        let request: QueryRequest<TerraQueryWrapper> = match from_slice(bin_request) {
            Ok(v) => v,
            Err(e) => {
                return Err(SystemError::InvalidRequest {
                    error: format!("Parsing query request: {}", e),
                    request: bin_request.into(),
                })
            }
        };
        self.handle_query(&request)
    }
}

#[derive(Clone, Default)]
pub struct LiquidationPercentQuerier {
    // this lets us iterate over all pairs that match the first string
    liquidation_percent: HashMap<HumanAddr, Decimal256>,
}

impl LiquidationPercentQuerier {
    pub fn new(liquidation_percent: &[(&HumanAddr, &Decimal256)]) -> Self {
        LiquidationPercentQuerier {
            liquidation_percent: liquidation_percent_to_map(liquidation_percent),
        }
    }
}

pub(crate) fn liquidation_percent_to_map(
    liquidation_percent: &[(&HumanAddr, &Decimal256)],
) -> HashMap<HumanAddr, Decimal256> {
    let mut liquidation_percent_map: HashMap<HumanAddr, Decimal256> = HashMap::new();
    for (liquidation_contract, liquidation_percent) in liquidation_percent.iter() {
        liquidation_percent_map.insert((*liquidation_contract).clone(), **liquidation_percent);
    }
    liquidation_percent_map
}

impl WasmMockQuerier {
    pub fn handle_query(&self, request: &QueryRequest<TerraQueryWrapper>) -> QuerierResult {
        match &request {
            QueryRequest::Custom(TerraQueryWrapper { route, query_data }) => {
                if &TerraRoute::Treasury == route {
                    match query_data {
                        TerraQuery::TaxRate {} => {
                            let res = TaxRateResponse {
                                rate: self.tax_querier.rate,
                            };
                            Ok(to_binary(&res))
                        }
                        TerraQuery::TaxCap { denom } => {
                            let cap = self
                                .tax_querier
                                .caps
                                .get(denom)
                                .copied()
                                .unwrap_or_default();
                            let res = TaxCapResponse { cap };
                            Ok(to_binary(&res))
                        }
                        _ => panic!("DO NOT ENTER HERE"),
                    }
                } else {
                    panic!("DO NOT ENTER HERE")
                }
            }
            QueryRequest::Wasm(WasmQuery::Smart { contract_addr, msg }) => {
                match from_binary(&msg).unwrap() {
                    QueryMsg::EpochState {
                        block_height: _,
                        distributed_interest: _,
                    } => match self.epoch_state_querier.epoch_state.get(&contract_addr) {
                        Some(v) => Ok(to_binary(&EpochStateResponse {
                            aterra_supply: v.0,
                            exchange_rate: v.1,
                        })),
                        None => Err(SystemError::InvalidRequest {
                            error: "No epoch state exists".to_string(),
                            request: msg.as_slice().into(),
                        }),
                    },
                    QueryMsg::BorrowerInfo {
                        borrower,
                        block_height: _,
                    } => match self.loan_amount_querier.borrower_amount.get(&borrower) {
                        Some(v) => Ok(to_binary(&BorrowerInfoResponse {
                            borrower,
                            interest_index: Decimal256::one(),
                            reward_index: Decimal256::zero(),
                            loan_amount: *v,
                            pending_rewards: Decimal256::zero(),
                        })),
                        None => Err(SystemError::InvalidRequest {
                            error: "No borrow amount exists".to_string(),
                            request: msg.as_slice().into(),
                        }),
                    },
                    QueryMsg::Price { base, quote } => {
                        match self.oracle_price_querier.oracle_price.get(&(base, quote)) {
                            Some(v) => Ok(to_binary(&PriceResponse {
                                rate: v.0,
                                last_updated_base: v.1,
                                last_updated_quote: v.2,
                            })),
                            None => Err(SystemError::InvalidRequest {
                                error: "No oracle price exists".to_string(),
                                request: msg.as_slice().into(),
                            }),
                        }
                    }
                    QueryMsg::LiquidationAmount {
                        borrow_amount,
                        borrow_limit,
                        collaterals,
                        collateral_prices: _,
                    } => {
                        match self
                            .liquidation_percent_querier
                            .liquidation_percent
                            .get(&contract_addr)
                        {
                            Some(v) => {
                                if borrow_amount > borrow_limit {
                                    Ok(to_binary(&LiquidationAmountResponse {
                                        collaterals: collaterals
                                            .iter()
                                            .map(|x| (x.0.clone(), x.1 * *v))
                                            .collect::<TokensHuman>()
                                            .to_vec(),
                                    }))
                                } else {
                                    Ok(to_binary(&LiquidationAmountResponse {
                                        collaterals: vec![],
                                    }))
                                }
                            }
                            None => Err(SystemError::InvalidRequest {
                                error: "No liquidation percent exists".to_string(),
                                request: msg.as_slice().into(),
                            }),
                        }
                    }
                }
            }
            _ => self.base.handle_query(request),
        }
    }
}

impl WasmMockQuerier {
    pub fn new<A: Api>(base: MockQuerier<TerraQueryWrapper>, _api: A) -> Self {
        WasmMockQuerier {
            base,
            tax_querier: TaxQuerier::default(),
            epoch_state_querier: EpochStateQuerier::default(),
            oracle_price_querier: OraclePriceQuerier::default(),
            loan_amount_querier: LoanAmountQuerier::default(),
            liquidation_percent_querier: LiquidationPercentQuerier::default(),
        }
    }

    // configure the tax mock querier
    pub fn with_tax(&mut self, rate: Decimal, caps: &[(&String, &Uint128)]) {
        self.tax_querier = TaxQuerier::new(rate, caps);
    }

    pub fn with_epoch_state(&mut self, epoch_state: &[(&HumanAddr, &(Uint256, Decimal256))]) {
        self.epoch_state_querier = EpochStateQuerier::new(epoch_state);
    }

    pub fn with_oracle_price(
        &mut self,
        oracle_price: &[(&(String, String), &(Decimal256, u64, u64))],
    ) {
        self.oracle_price_querier = OraclePriceQuerier::new(oracle_price);
    }

    pub fn with_loan_amount(&mut self, loan_amount: &[(&HumanAddr, &Uint256)]) {
        self.loan_amount_querier = LoanAmountQuerier::new(loan_amount);
    }

    pub fn with_liquidation_percent(&mut self, liquidation_percent: &[(&HumanAddr, &Decimal256)]) {
        self.liquidation_percent_querier = LiquidationPercentQuerier::new(liquidation_percent);
    }
}
