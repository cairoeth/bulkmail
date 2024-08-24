use alloy::primitives::{Address, BlockNumber, B256};
use alloy::rpc::types::{Block, TransactionReceipt};
use alloy::providers::{ PendingTransactionBuilder, Provider, ProviderBuilder, RootProvider, WsConnect};
use alloy::pubsub::{PubSubFrontend, Subscription};
use std::sync::Arc;
use alloy::consensus::{TxEip1559, TypedTransaction};
use alloy::network::{Ethereum, EthereumWallet, NetworkWallet};
use alloy::signers::k256::ecdsa::SigningKey;
use alloy::signers::local::PrivateKeySigner;
use alloy::transports::{RpcError, TransportErrorKind};
use thiserror::Error;
/// The main error type for the chain module
#[derive(Error, Debug)]
pub enum Error {
    #[error("RPC error: {0}")]
    Rpc(#[from] RpcError<TransportErrorKind>),
    #[error("subscription error: {0}")]
    Subscription(String),
    #[error("signing error: {0}")]
    Signing(#[from] alloy::signers::Error),
}

/// A wrapper around the Alloy RpcClient to interact with the Ethereum chain
#[derive(Clone)]
pub struct Chain {
    chain_id: u64,
    wallet: EthereumWallet,
    provider: Arc<RootProvider<PubSubFrontend>>,
}

impl Chain {
    pub async fn new(url: &str, signing_key: SigningKey, chain_id: u64) -> Result<Self, Error> {
        let ws = WsConnect::new(url);
        let signer = PrivateKeySigner::from_signing_key(signing_key);

        Ok(Self {
            chain_id,
            wallet: EthereumWallet::new(signer),
            provider: Arc::new(ProviderBuilder::new().on_ws(ws).await?),
        })
    }

    pub fn id(&self) -> u64 {
        self.chain_id
    }

    // pub fn height(&self) -> u64 {
    //     0
    // }

    /// Subscribe to new blocks
    pub async fn subscribe_new_blocks(&self) -> Result<Subscription<Block>, Error> {
        Ok(self.provider.subscribe_blocks().await?)
    }

    /// Get the current block number
    pub async fn get_block_number(&self) -> Result<BlockNumber, Error> {
        Ok(self.provider.get_block_number().await?)
    }

    /// Get the nonce for an account
    pub async fn get_account_nonce(&self, address: Address) -> Result<u64, Error> {
        Ok(self.provider.get_transaction_count(address).await?)
    }

    /// Get a transaction receipt
    pub async fn get_receipt(&self, tx_hash: B256) -> Result<Option<TransactionReceipt>, Error> {
        Ok(self.provider.get_transaction_receipt(tx_hash).await?)
    }

    /// Send a transaction and return a watcher
    pub async fn send_transaction(&self, tx: TxEip1559) -> Result<PendingTransactionBuilder<PubSubFrontend, Ethereum>, Error> {
        // Sign the transaction
        let envelope = <EthereumWallet as NetworkWallet<Ethereum>>::sign_transaction(
            &self.wallet,
            TypedTransaction::Eip1559(tx)
        ).await?;

        // Send it and return a watcher
        let pending = self.provider.send_tx_envelope(envelope).await?;
        Ok(pending)
     }

}
