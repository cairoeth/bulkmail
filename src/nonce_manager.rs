use crate::{chain::Chain, Error};
use std::collections::BTreeSet;
use tokio::sync::Mutex;
use alloy::primitives::Address;

pub(crate) struct NonceManager {
    chain: Chain,
    address: Address,
    current_nonce: Mutex<u64>,
    in_flight_nonces: Mutex<BTreeSet<u64>>,
}

impl NonceManager {
    pub async fn new(chain: Chain, address: Address) -> Result<Self, Error> {
        let current_nonce = chain.get_account_nonce(address).await?;

        Ok(Self {
            chain,
            address,
            current_nonce: Mutex::new(current_nonce),
            in_flight_nonces: Mutex::new(BTreeSet::new()),
        })
    }

    pub async fn get_next_available_nonce(&self) -> u64 {
        let mut current_nonce = self.current_nonce.lock().await;
        let mut in_flight_nonces = self.in_flight_nonces.lock().await;

        // Find the first non-used nonce
        while in_flight_nonces.contains(&current_nonce) {
            *current_nonce += 1;
        }

        let nonce = *current_nonce;
        // *current_nonce += 1;
        in_flight_nonces.insert(nonce);

        nonce
    }

    pub async fn mark_nonce_available(&self, nonce: u64) {
        self.in_flight_nonces.lock().await.remove(&nonce);
    }

    pub async fn update_current_nonce(&self, new_nonce: u64) {
        let mut current_nonce = self.current_nonce.lock().await;
        if new_nonce > *current_nonce {
            *current_nonce = new_nonce;
            // Remove all in-flight nonces less than the new current nonce
            let mut in_flight_nonces = self.in_flight_nonces.lock().await;
            in_flight_nonces.retain(|&n| n >= new_nonce);
        }
    }

    pub async fn sync_nonce(&self) -> Result<(), Error> {
        let on_chain_nonce = self.chain.get_account_nonce(self.address).await?;
        self.update_current_nonce(on_chain_nonce).await;
        Ok(())
    }
}
