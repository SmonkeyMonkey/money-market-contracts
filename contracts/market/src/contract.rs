use crate::borrow::{
    borrow_stable, claim_rewards, compute_interest, compute_interest_raw, compute_reward,
    query_borrower_info, query_borrower_infos, repay_stable, repay_stable_from_liquidation,
};
use crate::deposit::{compute_exchange_rate_raw, deposit_stable, redeem_stable};
use crate::migration::{migrate_config, migrate_state};
use crate::querier::{query_anc_emission_rate, query_borrow_rate, query_target_deposit_rate};
use crate::state::{read_config, read_state, store_config, store_state, Config, State};

use cosmwasm_bignumber::{Decimal256, Uint256};
use cosmwasm_std::{
    from_binary, log, to_binary, Api, BankMsg, Binary, CanonicalAddr, Coin, CosmosMsg, Env, Extern,
    HandleResponse, HandleResult, HumanAddr, InitResponse, InitResult, MigrateResponse,
    MigrateResult, Querier, StdError, StdResult, Storage, Uint128, WasmMsg,
};
use cw20::{Cw20CoinHuman, Cw20ReceiveMsg, MinterResponse};

use moneymarket::interest_model::BorrowRateResponse;
use moneymarket::market::{
    ConfigResponse, Cw20HookMsg, EpochStateResponse, HandleMsg, InitMsg, MigrateMsg, QueryMsg,
    StateResponse,
};
use moneymarket::querier::{deduct_tax, query_balance, query_supply};
use terraswap::hook::InitHook;
use terraswap::token::InitMsg as TokenInitMsg;

pub const INITIAL_DEPOSIT_AMOUNT: u128 = 1000000;
pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> InitResult {
    let initial_deposit = env
        .message
        .sent_funds
        .iter()
        .find(|c| c.denom == msg.stable_denom)
        .map(|c| c.amount)
        .unwrap_or_else(|| Uint128::zero());

    if initial_deposit != Uint128(INITIAL_DEPOSIT_AMOUNT) {
        return Err(StdError::generic_err(format!(
            "Must deposit initial funds {:?}{:?}",
            INITIAL_DEPOSIT_AMOUNT,
            msg.stable_denom.clone()
        )));
    }

    store_config(
        &mut deps.storage,
        &Config {
            contract_addr: deps.api.canonical_address(&env.contract.address)?,
            owner_addr: deps.api.canonical_address(&msg.owner_addr)?,
            aterra_contract: CanonicalAddr::default(),
            overseer_contract: CanonicalAddr::default(),
            interest_model: CanonicalAddr::default(),
            distribution_model: CanonicalAddr::default(),
            collector_contract: CanonicalAddr::default(),
            distributor_contract: CanonicalAddr::default(),
            stable_denom: msg.stable_denom.clone(),
            max_borrow_factor: msg.max_borrow_factor,
        },
    )?;

    store_state(
        &mut deps.storage,
        &State {
            total_liabilities: Decimal256::zero(),
            total_reserves: Decimal256::zero(),
            last_interest_updated: env.block.height,
            last_reward_updated: env.block.height,
            global_interest_index: Decimal256::one(),
            global_reward_index: Decimal256::zero(),
            anc_emission_rate: msg.anc_emission_rate,
            prev_aterra_supply: Uint256::zero(),
            prev_exchange_rate: Decimal256::one(),
        },
    )?;

    Ok(InitResponse {
        messages: vec![CosmosMsg::Wasm(WasmMsg::Instantiate {
            code_id: msg.aterra_code_id,
            send: vec![],
            label: None,
            msg: to_binary(&TokenInitMsg {
                name: format!("Anchor Terra {}", msg.stable_denom[1..].to_uppercase()),
                symbol: format!(
                    "a{}T",
                    msg.stable_denom[1..(msg.stable_denom.len() - 1)].to_uppercase()
                ),
                decimals: 6u8,
                initial_balances: vec![Cw20CoinHuman {
                    address: env.contract.address.clone(),
                    amount: Uint128(INITIAL_DEPOSIT_AMOUNT),
                }],
                mint: Some(MinterResponse {
                    minter: env.contract.address.clone(),
                    cap: None,
                }),
                init_hook: Some(InitHook {
                    contract_addr: env.contract.address,
                    msg: to_binary(&HandleMsg::RegisterATerra {})?,
                }),
            })?,
        })],
        log: vec![],
    })
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> HandleResult {
    match msg {
        HandleMsg::Receive(msg) => receive_cw20(deps, env, msg),
        HandleMsg::RegisterATerra {} => register_aterra(deps, env),
        HandleMsg::RegisterContracts {
            overseer_contract,
            interest_model,
            distribution_model,
            collector_contract,
            distributor_contract,
        } => register_contracts(
            deps,
            overseer_contract,
            interest_model,
            distribution_model,
            collector_contract,
            distributor_contract,
        ),
        HandleMsg::UpdateConfig {
            owner_addr,
            interest_model,
            distribution_model,
            max_borrow_factor,
        } => update_config(
            deps,
            env,
            owner_addr,
            interest_model,
            distribution_model,
            max_borrow_factor,
        ),
        HandleMsg::ExecuteEpochOperations {
            deposit_rate,
            target_deposit_rate,
            threshold_deposit_rate,
            distributed_interest,
        } => execute_epoch_operations(
            deps,
            env,
            deposit_rate,
            target_deposit_rate,
            threshold_deposit_rate,
            distributed_interest,
        ),
        HandleMsg::DepositStable {} => deposit_stable(deps, env),
        HandleMsg::BorrowStable { borrow_amount, to } => {
            borrow_stable(deps, env, borrow_amount, to)
        }
        HandleMsg::RepayStable {} => repay_stable(deps, env),
        HandleMsg::RepayStableFromLiquidation {
            borrower,
            prev_balance,
        } => repay_stable_from_liquidation(deps, env, borrower, prev_balance),
        HandleMsg::ClaimRewards { to } => claim_rewards(deps, env, to),
    }
}

pub fn receive_cw20<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    cw20_msg: Cw20ReceiveMsg,
) -> HandleResult {
    let contract_addr = env.message.sender.clone();
    if let Some(msg) = cw20_msg.msg {
        match from_binary(&msg)? {
            Cw20HookMsg::RedeemStable {} => {
                // only asset contract can execute this message
                let config: Config = read_config(&deps.storage)?;
                if deps.api.canonical_address(&contract_addr)? != config.aterra_contract {
                    return Err(StdError::unauthorized());
                }

                redeem_stable(deps, env, cw20_msg.sender, cw20_msg.amount)
            }
        }
    } else {
        Err(StdError::generic_err(
            "Invalid request: \"redeem stable\" message not included in request",
        ))
    }
}

pub fn register_aterra<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> HandleResult {
    let mut config: Config = read_config(&deps.storage)?;
    if config.aterra_contract != CanonicalAddr::default() {
        return Err(StdError::unauthorized());
    }

    config.aterra_contract = deps.api.canonical_address(&env.message.sender)?;
    store_config(&mut deps.storage, &config)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![log("aterra", env.message.sender)],
        data: None,
    })
}

pub fn register_contracts<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    overseer_contract: HumanAddr,
    interest_model: HumanAddr,
    distribution_model: HumanAddr,
    collector_contract: HumanAddr,
    distributor_contract: HumanAddr,
) -> HandleResult {
    let mut config: Config = read_config(&deps.storage)?;
    if config.overseer_contract != CanonicalAddr::default()
        || config.interest_model != CanonicalAddr::default()
        || config.distribution_model != CanonicalAddr::default()
        || config.collector_contract != CanonicalAddr::default()
        || config.distributor_contract != CanonicalAddr::default()
    {
        return Err(StdError::unauthorized());
    }

    config.overseer_contract = deps.api.canonical_address(&overseer_contract)?;
    config.interest_model = deps.api.canonical_address(&interest_model)?;
    config.distribution_model = deps.api.canonical_address(&distribution_model)?;
    config.collector_contract = deps.api.canonical_address(&collector_contract)?;
    config.distributor_contract = deps.api.canonical_address(&distributor_contract)?;
    store_config(&mut deps.storage, &config)?;

    Ok(HandleResponse::default())
}

pub fn update_config<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    owner_addr: Option<HumanAddr>,
    interest_model: Option<HumanAddr>,
    distribution_model: Option<HumanAddr>,
    max_borrow_factor: Option<Decimal256>,
) -> HandleResult {
    let mut config: Config = read_config(&deps.storage)?;

    // permission check
    if deps.api.canonical_address(&env.message.sender)? != config.owner_addr {
        return Err(StdError::unauthorized());
    }

    if let Some(owner_addr) = owner_addr {
        config.owner_addr = deps.api.canonical_address(&owner_addr)?;
    }

    if interest_model.is_some() {
        let mut state: State = read_state(&deps.storage)?;
        compute_interest(&deps, &config, &mut state, env.block.height, None)?;
        store_state(&mut deps.storage, &state)?;

        if let Some(interest_model) = interest_model {
            config.interest_model = deps.api.canonical_address(&interest_model)?;
        }
    }

    if let Some(distribution_model) = distribution_model {
        config.distribution_model = deps.api.canonical_address(&distribution_model)?;
    }

    if let Some(max_borrow_factor) = max_borrow_factor {
        config.max_borrow_factor = max_borrow_factor;
    }

    store_config(&mut deps.storage, &config)?;
    Ok(HandleResponse {
        messages: vec![],
        log: vec![log("action", "update_config")],
        data: None,
    })
}

pub fn execute_epoch_operations<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    deposit_rate: Decimal256,
    target_deposit_rate: Decimal256,
    threshold_deposit_rate: Decimal256,
    distributed_interest: Uint256,
) -> HandleResult {
    let config: Config = read_config(&deps.storage)?;
    if config.overseer_contract != deps.api.canonical_address(&env.message.sender)? {
        return Err(StdError::unauthorized());
    }

    let mut state: State = read_state(&deps.storage)?;

    // Compute interest and reward before updating anc_emission_rate
    let aterra_supply = query_supply(&deps, &deps.api.human_address(&config.aterra_contract)?)?;
    let balance: Uint256 = query_balance(
        &deps,
        &deps.api.human_address(&config.contract_addr)?,
        config.stable_denom.to_string(),
    )? - distributed_interest;

    let borrow_rate_res: BorrowRateResponse = query_borrow_rate(
        &deps,
        &deps.api.human_address(&config.interest_model)?,
        balance,
        state.total_liabilities,
        state.total_reserves,
    )?;

    compute_interest_raw(
        &mut state,
        env.block.height,
        balance,
        aterra_supply,
        borrow_rate_res.rate,
        target_deposit_rate,
    );

    // recompute prev_exchange_rate with distributed_interest
    state.prev_exchange_rate =
        compute_exchange_rate_raw(&state, aterra_supply, balance + distributed_interest);

    compute_reward(&mut state, env.block.height);

    // Compute total_reserves to fund collector contract
    // Update total_reserves and send it to collector contract
    // only when there is enough balance
    let total_reserves = state.total_reserves * Uint256::one();
    let messages: Vec<CosmosMsg> = if !total_reserves.is_zero() && balance > total_reserves {
        state.total_reserves = state.total_reserves - Decimal256::from_uint256(total_reserves);

        vec![CosmosMsg::Bank(BankMsg::Send {
            from_address: env.contract.address,
            to_address: deps.api.human_address(&config.collector_contract)?,
            amount: vec![deduct_tax(
                &deps,
                Coin {
                    denom: config.stable_denom,
                    amount: total_reserves.into(),
                },
            )?],
        })]
    } else {
        vec![]
    };

    // Query updated anc_emission_rate
    state.anc_emission_rate = query_anc_emission_rate(
        &deps,
        &deps.api.human_address(&config.distribution_model)?,
        deposit_rate,
        target_deposit_rate,
        threshold_deposit_rate,
        state.anc_emission_rate,
    )?
    .emission_rate;

    store_state(&mut deps.storage, &state)?;

    return Ok(HandleResponse {
        messages,
        log: vec![
            log("action", "execute_epoch_operations"),
            log("total_reserves", total_reserves),
            log("anc_emission_rate", state.anc_emission_rate),
        ],
        data: None,
    });
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
        QueryMsg::State { block_height } => to_binary(&query_state(deps, block_height)?),
        QueryMsg::EpochState {
            block_height,
            distributed_interest,
        } => to_binary(&query_epoch_state(
            deps,
            block_height,
            distributed_interest,
        )?),
        QueryMsg::BorrowerInfo {
            borrower,
            block_height,
        } => to_binary(&query_borrower_info(deps, borrower, block_height)?),
        QueryMsg::BorrowerInfos { start_after, limit } => {
            to_binary(&query_borrower_infos(deps, start_after, limit)?)
        }
    }
}

pub fn query_config<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<ConfigResponse> {
    let config: Config = read_config(&deps.storage)?;
    Ok(ConfigResponse {
        owner_addr: deps.api.human_address(&config.owner_addr)?,
        aterra_contract: deps.api.human_address(&config.aterra_contract)?,
        interest_model: deps.api.human_address(&config.interest_model)?,
        distribution_model: deps.api.human_address(&config.distribution_model)?,
        overseer_contract: deps.api.human_address(&config.overseer_contract)?,
        collector_contract: deps.api.human_address(&config.collector_contract)?,
        distributor_contract: deps.api.human_address(&config.distributor_contract)?,
        stable_denom: config.stable_denom,
        max_borrow_factor: config.max_borrow_factor,
    })
}

pub fn query_state<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    block_height: Option<u64>,
) -> StdResult<StateResponse> {
    let mut state: State = read_state(&deps.storage)?;

    if let Some(block_height) = block_height {
        if block_height < state.last_interest_updated {
            return Err(StdError::generic_err(
                "block_height must bigger than last_interest_updated",
            ));
        }

        if block_height < state.last_reward_updated {
            return Err(StdError::generic_err(
                "block_height must bigger than last_reward_updated",
            ));
        }

        let config: Config = read_config(&deps.storage)?;

        // Compute interest rate with given block height
        compute_interest(&deps, &config, &mut state, block_height, None)?;

        // Compute reward rate with given block height
        compute_reward(&mut state, block_height);
    }

    Ok(StateResponse {
        total_liabilities: state.total_liabilities,
        total_reserves: state.total_reserves,
        last_interest_updated: state.last_interest_updated,
        last_reward_updated: state.last_reward_updated,
        global_interest_index: state.global_interest_index,
        global_reward_index: state.global_reward_index,
        anc_emission_rate: state.anc_emission_rate,
        prev_aterra_supply: state.prev_aterra_supply,
        prev_exchange_rate: state.prev_exchange_rate,
    })
}

pub fn query_epoch_state<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    block_height: Option<u64>,
    distributed_interest: Option<Uint256>,
) -> StdResult<EpochStateResponse> {
    let config: Config = read_config(&deps.storage)?;
    let mut state: State = read_state(&deps.storage)?;

    let distributed_interest = distributed_interest.unwrap_or(Uint256::zero());
    let aterra_supply = query_supply(&deps, &deps.api.human_address(&config.aterra_contract)?)?;
    let balance = query_balance(
        &deps,
        &deps.api.human_address(&config.contract_addr)?,
        config.stable_denom.to_string(),
    )? - distributed_interest;

    let exchange_rate = if let Some(block_height) = block_height {
        if block_height < state.last_interest_updated {
            return Err(StdError::generic_err(
                "block_height must bigger than last_interest_updated",
            ));
        }

        let borrow_rate_res: BorrowRateResponse = query_borrow_rate(
            &deps,
            &deps.api.human_address(&config.interest_model)?,
            balance,
            state.total_liabilities,
            state.total_reserves,
        )?;

        let target_deposit_rate: Decimal256 =
            query_target_deposit_rate(&deps, &deps.api.human_address(&config.overseer_contract)?)?;

        // Compute interest rate to return latest epoch state
        compute_interest_raw(
            &mut state,
            block_height,
            balance,
            aterra_supply,
            borrow_rate_res.rate,
            target_deposit_rate,
        );

        // compute_interest_raw store current exchange rate
        // as prev_exchange_rate, so just return prev_exchange_rate
        compute_exchange_rate_raw(&state, aterra_supply, balance + distributed_interest)
    } else {
        compute_exchange_rate_raw(&state, aterra_supply, balance + distributed_interest)
    };

    Ok(EpochStateResponse {
        exchange_rate,
        aterra_supply,
    })
}

pub fn migrate<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: MigrateMsg,
) -> MigrateResult {
    // migrate config to use new Config
    // also update collector_contract to the given address
    migrate_config(
        &mut deps.storage,
        deps.api.canonical_address(&msg.collector_contract)?,
    )?;

    let config: Config = read_config(&deps.storage)?;
    let aterra_supply = query_supply(&deps, &deps.api.human_address(&config.aterra_contract)?)?;
    let balance = query_balance(&deps, &env.contract.address, config.stable_denom)?;

    // migrate state to use new State
    migrate_state(&mut deps.storage, aterra_supply, balance)?;

    Ok(MigrateResponse::default())
}
