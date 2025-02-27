use crate::state::{
    read_config, read_feeder, read_price, read_prices, store_config, store_feeder, store_price,
    Config, PriceInfo,
};

use cosmwasm_bignumber::Decimal256;
use cosmwasm_std::{
    log, to_binary, Api, Binary, Env, Extern, HandleResponse, HandleResult, HumanAddr,
    InitResponse, MigrateResponse, MigrateResult, Querier, StdError, StdResult, Storage,
};

use moneymarket::oracle::{
    ConfigResponse, FeederResponse, HandleMsg, InitMsg, MigrateMsg, PriceResponse, PricesResponse,
    PricesResponseElem, QueryMsg,
};

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    store_config(
        &mut deps.storage,
        &Config {
            owner: deps.api.canonical_address(&msg.owner)?,
            base_asset: msg.base_asset,
        },
    )?;

    Ok(InitResponse::default())
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> HandleResult {
    match msg {
        HandleMsg::UpdateConfig { owner } => update_config(deps, env, owner),
        HandleMsg::RegisterFeeder { asset, feeder } => register_feeder(deps, env, asset, feeder),
        HandleMsg::FeedPrice { prices } => feed_prices(deps, env, prices),
    }
}

pub fn update_config<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    owner: Option<HumanAddr>,
) -> HandleResult {
    let mut config: Config = read_config(&deps.storage)?;
    if deps.api.canonical_address(&env.message.sender)? != config.owner {
        return Err(StdError::unauthorized());
    }

    if let Some(owner) = owner {
        config.owner = deps.api.canonical_address(&owner)?;
    }

    store_config(&mut deps.storage, &config)?;
    Ok(HandleResponse::default())
}

pub fn register_feeder<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    asset: String,
    feeder: HumanAddr,
) -> HandleResult {
    let config: Config = read_config(&deps.storage)?;
    if deps.api.canonical_address(&env.message.sender)? != config.owner {
        return Err(StdError::unauthorized());
    }

    store_feeder(
        &mut deps.storage,
        &asset,
        &deps.api.canonical_address(&feeder)?,
    )?;

    return Ok(HandleResponse {
        messages: vec![],
        log: vec![
            log("action", "register_feeder"),
            log("asset", asset),
            log("feeder", feeder),
        ],
        data: None,
    });
}

pub fn feed_prices<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    prices: Vec<(String, Decimal256)>,
) -> HandleResult {
    let mut logs = vec![log("action", "feed_prices")];
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    for price in prices {
        let asset: String = price.0;
        let price: Decimal256 = price.1;

        // Check feeder permission
        let feeder = read_feeder(&deps.storage, &asset)?;
        if feeder != sender_raw {
            return Err(StdError::unauthorized());
        }

        logs.push(log("asset", asset.to_string()));
        logs.push(log("price", price));

        store_price(
            &mut deps.storage,
            &asset,
            &PriceInfo {
                last_updated_time: env.block.time,
                price,
            },
        )?;
    }

    Ok(HandleResponse {
        messages: vec![],
        log: logs,
        data: None,
    })
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
        QueryMsg::Feeder { asset } => to_binary(&query_feeder(deps, asset)?),
        QueryMsg::Price { base, quote } => to_binary(&query_price(deps, base, quote)?),
        QueryMsg::Prices { start_after, limit } => {
            to_binary(&query_prices(deps, start_after, limit)?)
        }
    }
}

fn query_config<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<ConfigResponse> {
    let state = read_config(&deps.storage)?;
    let resp = ConfigResponse {
        owner: deps.api.human_address(&state.owner)?,
        base_asset: state.base_asset,
    };

    Ok(resp)
}

fn query_feeder<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    asset: String,
) -> StdResult<FeederResponse> {
    let feeder = read_feeder(&deps.storage, &asset)?;
    let resp = FeederResponse {
        asset,
        feeder: deps.api.human_address(&feeder)?,
    };

    Ok(resp)
}

fn query_price<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    base: String,
    quote: String,
) -> StdResult<PriceResponse> {
    let config: Config = read_config(&deps.storage)?;
    let quote_price = if config.base_asset == quote {
        PriceInfo {
            price: Decimal256::one(),
            last_updated_time: 9999999999,
        }
    } else {
        read_price(&deps.storage, &quote)?
    };

    let base_price = if config.base_asset == base {
        PriceInfo {
            price: Decimal256::one(),
            last_updated_time: 9999999999,
        }
    } else {
        read_price(&deps.storage, &base)?
    };

    Ok(PriceResponse {
        rate: base_price.price / quote_price.price,
        last_updated_base: base_price.last_updated_time,
        last_updated_quote: quote_price.last_updated_time,
    })
}

fn query_prices<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<PricesResponse> {
    let prices: Vec<PricesResponseElem> = read_prices(&deps.storage, start_after, limit)?;
    Ok(PricesResponse { prices })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env};
    use cosmwasm_std::{from_binary, StdError};
    use std::str::FromStr;

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies(20, &[]);

        let msg = InitMsg {
            owner: HumanAddr("owner0000".to_string()),
            base_asset: "base0000".to_string(),
        };

        let env = mock_env("addr0000", &[]);

        // we can just call .unwrap() to assert this was a success
        let res = init(&mut deps, env, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let value = query_config(&deps).unwrap();
        assert_eq!("owner0000", value.owner.as_str());
        assert_eq!("base0000", &value.base_asset.to_string());
    }

    #[test]
    fn update_config() {
        let mut deps = mock_dependencies(20, &[]);

        let msg = InitMsg {
            owner: HumanAddr("owner0000".to_string()),
            base_asset: "base0000".to_string(),
        };

        let env = mock_env("addr0000", &[]);
        let _res = init(&mut deps, env, msg).unwrap();

        // update owner
        let env = mock_env("owner0000", &[]);
        let msg = HandleMsg::UpdateConfig {
            owner: Some(HumanAddr("owner0001".to_string())),
        };

        let res = handle(&mut deps, env, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let value = query_config(&deps).unwrap();
        assert_eq!("owner0001", value.owner.as_str());
        assert_eq!("base0000", &value.base_asset.to_string());

        // Unauthorized err
        let env = mock_env("owner0000", &[]);
        let msg = HandleMsg::UpdateConfig { owner: None };

        let res = handle(&mut deps, env, msg);
        match res {
            Err(StdError::Unauthorized { .. }) => {}
            _ => panic!("Must return unauthorized error"),
        }
    }

    #[test]
    fn register_feeder() {
        let mut deps = mock_dependencies(20, &[]);

        let msg = InitMsg {
            owner: HumanAddr("owner0000".to_string()),
            base_asset: "base0000".to_string(),
        };

        let env = mock_env("addr0000", &[]);
        let _res = init(&mut deps, env, msg).unwrap();

        let env = mock_env("addr0000", &[]);
        let msg = HandleMsg::RegisterFeeder {
            asset: "mAAPL".to_string(),
            feeder: HumanAddr::from("feeder0000"),
        };

        let res = handle(&mut deps, env, msg.clone());
        match res {
            Err(StdError::Unauthorized { .. }) => {}
            _ => panic!("DO NOT ENTER HERE"),
        }

        let env = mock_env("owner0000", &[]);
        let _res = handle(&mut deps, env, msg).unwrap();
        let feeder_res: FeederResponse = from_binary(
            &query(
                &deps,
                QueryMsg::Feeder {
                    asset: "mAAPL".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            feeder_res,
            FeederResponse {
                asset: "mAAPL".to_string(),
                feeder: HumanAddr::from("feeder0000"),
            }
        );
    }

    #[test]
    fn feed_price() {
        let mut deps = mock_dependencies(20, &[]);

        let msg = InitMsg {
            owner: HumanAddr("owner0000".to_string()),
            base_asset: "base0000".to_string(),
        };

        let env = mock_env("addr0000", &[]);
        let _res = init(&mut deps, env, msg).unwrap();

        // Register feeder for mAAPL
        let msg = HandleMsg::RegisterFeeder {
            asset: "mAAPL".to_string(),
            feeder: HumanAddr::from("feeder0000"),
        };
        let env = mock_env("owner0000", &[]);
        let _res = handle(&mut deps, env.clone(), msg).unwrap();

        // Register feeder for mGOGL
        let msg = HandleMsg::RegisterFeeder {
            asset: "mGOGL".to_string(),
            feeder: HumanAddr::from("feeder0000"),
        };
        let _res = handle(&mut deps, env.clone(), msg).unwrap();

        // Feed prices
        let env = mock_env("feeder0000", &[]);
        let msg = HandleMsg::FeedPrice {
            prices: vec![
                ("mAAPL".to_string(), Decimal256::from_str("1.2").unwrap()),
                ("mGOGL".to_string(), Decimal256::from_str("2.2").unwrap()),
            ],
        };
        let _res = handle(&mut deps, env.clone(), msg).unwrap();
        let value: PriceResponse =
            query_price(&deps, "mAAPL".to_string(), "base0000".to_string()).unwrap();
        assert_eq!(
            value,
            PriceResponse {
                rate: Decimal256::from_str("1.2").unwrap(),
                last_updated_base: env.block.time,
                last_updated_quote: 9999999999,
            }
        );

        let value: PriceResponse =
            query_price(&deps, "mGOGL".to_string(), "mAAPL".to_string()).unwrap();
        assert_eq!(
            value,
            PriceResponse {
                rate: Decimal256::from_str("1.833333333333333333").unwrap(),
                last_updated_base: env.block.time,
                last_updated_quote: env.block.time,
            }
        );

        let value: PricesResponse = query_prices(&deps, None, None).unwrap();
        assert_eq!(
            value,
            PricesResponse {
                prices: vec![
                    PricesResponseElem {
                        asset: "mAAPL".to_string(),
                        price: Decimal256::from_str("1.2").unwrap(),
                        last_updated_time: env.block.time,
                    },
                    PricesResponseElem {
                        asset: "mGOGL".to_string(),
                        price: Decimal256::from_str("2.2").unwrap(),
                        last_updated_time: env.block.time,
                    }
                ],
            }
        );

        // Unauthorized try
        let env = mock_env("addr0001", &[]);
        let msg = HandleMsg::FeedPrice {
            prices: vec![("mAAPL".to_string(), Decimal256::from_str("1.2").unwrap())],
        };

        let res = handle(&mut deps, env, msg);
        match res {
            Err(StdError::Unauthorized { .. }) => {}
            _ => panic!("Must return unauthorized error"),
        }
    }
}

pub fn migrate<S: Storage, A: Api, Q: Querier>(
    _deps: &mut Extern<S, A, Q>,
    _env: Env,
    _msg: MigrateMsg,
) -> MigrateResult {
    Ok(MigrateResponse::default())
}
