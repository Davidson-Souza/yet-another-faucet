use std::{env, sync::Mutex};

use anyhow::Result;
use cln_rpc::{
    model::requests::GetinfoRequest,
    primitives::{Amount, AmountOrAll, PublicKey},
    Response,
};

use crate::api::Error;
pub struct CLNDaemon {
    rpc: Mutex<cln_rpc::ClnRpc>,
    channel_lease_value: Amount,
    channel_lease_push: Amount,
}

impl CLNDaemon {
    pub async fn new(mut rpc: cln_rpc::ClnRpc) -> Result<Self> {
        let Response::Getinfo(res) = rpc
            .call(cln_rpc::Request::Getinfo(GetinfoRequest {}))
            .await?
        else {
            panic!("what?");
        };

        let channel_lease_value = env::var("CHANNEL_VALUE")
            .map(|value| value.parse().unwrap_or_default())
            .unwrap_or(1_000_000);
        let channel_lease_push = env::var("PUSH_VALUE")
            .map(|value| value.parse().unwrap_or_default())
            .unwrap_or(1_000_000);

        Ok(Self {
            rpc: Mutex::new(rpc),
            channel_lease_push: Amount::from_sat(channel_lease_push),
            channel_lease_value: Amount::from_sat(channel_lease_value),
        })
    }

    #[cfg(feature = "ln")]
    pub async fn open_channel(&self, id: PublicKey) -> Result<String, crate::api::Error> {
        let res = self
            .rpc
            .lock()
            .unwrap()
            .call(cln_rpc::Request::FundChannel(
                cln_rpc::model::requests::FundchannelRequest {
                    id,
                    amount: AmountOrAll::Amount(self.channel_lease_value),
                    feerate: None,
                    announce: Some(true),
                    minconf: Some(0),
                    push_msat: Some(self.channel_lease_push),
                    close_to: None,
                    request_amt: None,
                    compact_lease: None,
                    utxos: None,
                    mindepth: None,
                    reserve: None,
                },
            ))
            .await
            .map_err(|e| Error::CLNError(e.to_string()))?;
        let Response::FundChannel(channel_result) = res else {
            panic!("what?")
        };
        Ok(channel_result.channel_id)
    }
}
