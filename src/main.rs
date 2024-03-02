extern crate bitcoincore_rpc;
mod api;

#[cfg(feature = "ln")]
mod open_channel;

use std::{env, process::exit, str::FromStr};

use bitcoin::{Address, Amount};
use bitcoincore_rpc::{Auth, Client};

#[cfg(feature = "ln")]
use cln_rpc::ClnRpc;

#[cfg(feature = "ln")]
use open_channel::CLNDaemon;

#[actix::main]
async fn main() -> anyhow::Result<()> {
    let Ok(cookie_file) = env::var("BITCOIND_COOKIE_FILE") else {
        println!("cookie file not set");
        exit(1);
    };

    let url = env::var("BITCOIND_URL").unwrap_or("http://localhost:38332".into());

    let rpc = Client::new(&url, Auth::CookieFile(cookie_file.into()))?;

    let Ok(Ok(change)) = env::var("CHANGE_ADDRESS").map(|address| {
        Address::from_str(&address).and_then(|address| Ok(address.assume_checked()))
    }) else {
        println!(
            "You have to provide a valid change address. \n Please set the CHANGE_ADDRESS env var"
        );
        exit(1);
    };

    #[cfg(feature = "ln")]
    let Ok(cln_rpc) = env::var("CLN_RPC_DIR") else {
        println!("You have to provide the CLN_RPC_DIR");
        exit(1);
    };

    let max_sendable: Amount = match env::var("MAX_SENDABLE_AMOUNT").map(|amount| amount.parse()) {
        Ok(Ok(value)) => {
            println!("MAX_SENDABLE_AMOUNT set to {value}");
            value
        }
        Ok(Err(e)) => {
            println!("error parsing the MAX_SENDABLE_AMOUNT {e}, using default of 1_000_000");
            Amount::from_sat(1_000_000)
        }
        Err(_) => {
            println!("MAX_SENDABLE_AMOUNTA not set, using default of 1_000_000");
            Amount::from_sat(1_000_000)
        }
    };

    let min_sendable: Amount = match env::var("MIN_SENDABLE_AMOUNT").map(|amount| amount.parse()) {
        Ok(Ok(value)) => {
            println!("MIN_SENDABLE_AMOUNT set to {value}");
            value
        }
        Ok(Err(e)) => {
            println!("error parsing the MIN_SENDABLE_AMOUNT {e}, using default of 420");
            Amount::from_sat(420)
        }
        Err(_) => {
            println!("MIN_SENDABLE_AMOUNT not set, uing default of 420");
            Amount::from_sat(420)
        }
    };

    #[cfg(feature = "ln")]
    {
        let cln_rpc = ClnRpc::new(cln_rpc).await?;
        let cln = CLNDaemon::new(cln_rpc).await?;
        api::create_api(rpc, cln, max_sendable, min_sendable, change).await?;
    }

    #[cfg(not(feature = "ln"))]
    api::create_api(rpc, max_sendable, min_sendable, change).await?;

    Ok(())
}
