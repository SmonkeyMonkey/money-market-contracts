mod bid;
pub mod contract;
pub mod state;

#[cfg(test)]
mod testing;

#[cfg(all(target_arch = "wasm32", not(feature = "library")))]
cosmwasm_std::create_entry_points!(contract);
