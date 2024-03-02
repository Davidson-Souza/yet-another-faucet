//SPDX-License-Identifier: MIT

//! This is a simple REST API that can be used to query Utreexo data. You can get the roots
//! of the accumulator, get a proof for a leaf, and get a block and the associated UData.

use std::collections::HashMap;
use std::fmt::Display;
use std::str::FromStr;

use actix_cors::Cors;
use actix_web::http::StatusCode;
use actix_web::web;
use actix_web::App;
use actix_web::HttpResponse;
use actix_web::HttpServer;
use actix_web::ResponseError;
use bitcoin::Address;
use bitcoin::Amount;

use bitcoincore_rpc::{bitcoincore_rpc_json::CreateRawTransactionInput, Client, RpcApi};
#[cfg(feature = "ln")]
use cln_rpc::primitives::PublicKey;
use serde::Deserialize;

#[cfg(feature = "ln")]
use crate::open_channel::CLNDaemon;

struct AppState {
    rpc: Client,
    change_address: Address,
    max_sendable_amount: Amount,
    min_sendable_amount: Amount,
    #[cfg(feature = "ln")]
    cln: CLNDaemon,
}

#[derive(Debug)]
pub enum Error {
    /// This is a generic error with our bitcoin core
    JsonRpcNotWorking,
    /// We ran out of money and can't fulfill this request
    OutOfMoney,
    /// The provided address is invalid
    InvalidAddress,
    /// The user is asking for too much money
    AmountTooLarge,
    /// The user is ask for a amount too little
    Dust,
    #[cfg(feature = "ln")]
    CLNError(String),
}

impl From<bitcoincore_rpc::Error> for Error {
    fn from(_value: bitcoincore_rpc::Error) -> Self {
        Error::JsonRpcNotWorking
    }
}

/// The data passed to /send/
///
/// This is a POST route that will send `amount` to `address`
#[derive(Deserialize)]
pub struct SendMoney {
    address: String,
    amount: u64,
}

/// The data passed to the openchannel route
///
/// This will open a fixed-size channel to a node with `node_id`
#[cfg(feature = "ln")]
#[derive(Deserialize)]
struct GetChannel {
    node_id: PublicKey,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::JsonRpcNotWorking => write!(f, "our bitcoin core isn't working"),
            Error::OutOfMoney => write!(f, "we ran out of money, sorry :/"),
            Error::InvalidAddress => write!(f, "the provided address is invalid"),
            Error::AmountTooLarge => write!(f, "the request amount is too large"),
            Error::Dust => write!(f, "the requested amount is too little"),
            #[cfg(feature = "ln")]
            Error::CLNError(s) => write!(f, "some cln error: {s}"),
        }
    }
}

impl ResponseError for Error {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            Error::JsonRpcNotWorking => StatusCode::from_u16(500).unwrap(),
            Error::OutOfMoney => StatusCode::from_u16(500).unwrap(),
            Error::InvalidAddress => StatusCode::from_u16(400).unwrap(),
            Error::AmountTooLarge => StatusCode::from_u16(400).unwrap(),
            Error::Dust => StatusCode::from_u16(400).unwrap(),
            #[cfg(feature = "ln")]
            Error::CLNError(_) => StatusCode::from_u16(400).unwrap(),
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse<actix_web::body::BoxBody> {
        match self {
            Error::JsonRpcNotWorking => HttpResponse::InternalServerError().into(),
            Error::OutOfMoney => HttpResponse::InternalServerError()
                .body("We don't have enough money to handle this request right now\n")
                .into(),
            Error::InvalidAddress => HttpResponse::BadRequest()
                .body("The informed address is not a valid bitcoin address\n")
                .into(),
            Error::AmountTooLarge => {
                HttpResponse::BadRequest().body("The requested amount is too big\n")
            }
            Error::Dust => HttpResponse::BadRequest().body("The requested amount is too little\n"),
            #[cfg(feature = "ln")]
            Error::CLNError(e) => {
                HttpResponse::BadRequest().body(format!("Some problem with cln {e}"))
            }
        }
    }
}

#[cfg(feature = "ln")]
async fn open_channel(
    params: web::Json<GetChannel>,
    data: web::Data<AppState>,
) -> Result<String, Error> {
    let GetChannel { node_id } = params.into_inner();
    let cln = &data.cln;

    cln.open_channel(node_id).await
}

async fn send_to_address(
    params: web::Json<SendMoney>,
    data: web::Data<AppState>,
) -> Result<String, Error> {
    let rpc = &data.rpc;
    let SendMoney { address, amount } = params.into_inner();

    let amount = Amount::from_sat(amount);

    Address::from_str(&address)
        .map_err(|_| Error::InvalidAddress)?
        .require_network(bitcoin::Network::Signet)
        .map_err(|_| Error::InvalidAddress)?
        .to_string();

    if amount > data.max_sendable_amount {
        return Err(Error::AmountTooLarge);
    }

    if amount > data.min_sendable_amount {
        return Err(Error::Dust);
    }

    let mut unspents = rpc.list_unspent(None, None, None, None, None)?;
    let mut available = 0;
    let mut inputs = vec![];

    while available < (amount.to_sat() + 1_000) {
        let unspent = unspents.pop().ok_or(Error::OutOfMoney)?;
        let utxo = CreateRawTransactionInput {
            sequence: None,
            txid: unspent.txid,
            vout: unspent.vout,
        };

        inputs.push(utxo);

        available += unspent.amount.to_sat();
    }

    let mut outs = HashMap::new();

    outs.insert(address, amount);

    // change
    outs.insert(
        data.change_address.to_string(),
        Amount::from_sat(available - (amount.to_sat() + 1_000)),
    );

    let raw_tx = rpc.create_raw_transaction(&inputs, &outs, None, Some(true))?;
    let raw_tx = rpc
        .sign_raw_transaction_with_wallet(&raw_tx, None, None)?
        .transaction()
        .map_err(|_| Error::JsonRpcNotWorking)?;

    Ok(rpc
        .send_raw_transaction(&raw_tx)
        .map(|txid| txid.to_string() + "\n")?)
}

pub async fn index() -> HttpResponse {
    let body = std::fs::read_to_string("static/index.html").unwrap();
    HttpResponse::Ok().body(body)
}

#[cfg(feature = "ln")]
/// This function creates the actix-web server and returns a future that can be awaited.
pub async fn create_api(
    client: Client,
    cln: CLNDaemon,
    max_sendable_amount: Option<Amount>,
    min_sendable_amount: Option<Amount>,
    change_address: Address,
) -> std::io::Result<()> {
    let app_state = web::Data::new(AppState {
        rpc: client,
        cln,
        min_sendable_amount,
        max_sendable_amount,
        change_address,
    });

    HttpServer::new(move || {
        let cors = Cors::permissive();
        App::new()
            .wrap(cors)
            .app_data(app_state.clone())
            .route("/send/", web::post().to(send_to_address))
            .route("/channel/", web::post().to(open_channel))
            .route("/", web::get().to(index))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}

#[cfg(not(feature = "ln"))]
/// This function creates the actix-web server and returns a future that can be awaited.
pub async fn create_api(
    client: Client,
    max_sendable_amount: Amount,
    min_sendable_amount: Amount,
    change_address: Address,
) -> std::io::Result<()> {
    let app_state = web::Data::new(AppState {
        rpc: client,
        min_sendable_amount,
        max_sendable_amount,
        change_address,
    });

    HttpServer::new(move || {
        let cors = Cors::permissive();
        App::new()
            .wrap(cors)
            .app_data(app_state.clone())
            .route("/send/", web::post().to(send_to_address))
            .route("/", web::get().to(index))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
