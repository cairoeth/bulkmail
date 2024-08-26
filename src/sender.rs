use crate::{chain, chain::Chain, Error, GasPriceManager, Message, NonceManager, PriorityQueue};
use alloy::consensus::TxEip1559;
use alloy::network::Ethereum;
use alloy::primitives::{TxHash, TxKind};
use alloy::providers::PendingTransactionBuilder;
use alloy::pubsub::PubSubFrontend;
use alloy::transports::RpcError::ErrorResp;
use alloy::{
    primitives::{Address, B256},
    rpc::types::TransactionReceipt,
};
use futures::future::join;
use log::{debug, error, info};
use std::time::Duration;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::{Mutex, Semaphore};

const MAX_IN_FLIGHT_TRANSACTIONS: usize = 16;
const MAX_REPLACEMENTS: u32 = 3;
const GAS_PRICE_INCREASE_PERCENT: u8 = 20;
const TX_TIMEOUT: u64 = 3;

const TX_FAILURE_INSUFFICIENT_FUNDS: i64 = -32003;

type PendingMap = HashMap<TxHash, PendingTransaction>;
type SharedPendingMap = Arc<Mutex<PendingMap>>;
type SharedQueue = Arc<Mutex<PriorityQueue>>;

#[derive(Clone)]
struct PendingTransaction {
    msg: Message,
    created_at: Instant,
    replacement_count: u32,
    priority_fee: u128,
    nonce: u64,
}

#[derive(Clone)]
pub struct Sender {
    chain: Chain,
    nonce_manager: Arc<NonceManager>,
    gas_manager: Arc<GasPriceManager>,

    queue: SharedQueue,
    pending: SharedPendingMap,
    max_in_flight: Arc<Semaphore>,
}

impl Sender {
    pub async fn new(chain: Chain, address: Address) -> Result<Self, Error> {
        Ok(Self {
            chain: chain.clone(),
            nonce_manager: Arc::new(NonceManager::new(chain.clone(), address).await?),
            gas_manager: Arc::new(GasPriceManager::new()),

            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            max_in_flight: Arc::new(Semaphore::new(MAX_IN_FLIGHT_TRANSACTIONS)),
        })
    }

    pub async fn add_message(&self, msg: Message) {
        debug!("adding message {} {}", msg.to.unwrap(), msg.value.to_string());
        self.queue.lock().await.push(msg)
    }

    pub async fn run(&self) -> Result<(), Error> {
        let mut block_stream = self.chain.subscribe_new_blocks().await?;

        loop {
            tokio::select! {
                biased;

                block = block_stream.recv() => {
                    if let Err(e) = block {
                        error!("Error receiving block notification: {:?}", e);
                        continue
                    }
                    debug!("Received new block notification {}", block.unwrap().header.number.unwrap());

                    // Every block re-sync our nonce
                    self.nonce_manager.sync_nonce().await?
                }

                _ = self.process_next_message() => {}
            }
        }
    }

    async fn process_next_message(&self) {
        // Block until we have a slot available
        let permit = self.max_in_flight.clone().acquire_owned().await.unwrap();

        // Now see if there are any messages
        if let Some(msg) = self.queue.lock().await.pop() {
            // We have a message, process it in a separate task
            let sender = self.clone();
            tokio::spawn(async move {
                if let Err(Error::ChainError(chain::Error::Rpc(ErrorResp(e)))) = sender.process_message(msg).await {
                    if e.code == TX_FAILURE_INSUFFICIENT_FUNDS {
                        error!("Insufficient funds to send transaction; dropping message");
                    }
                }
            });
        } else {
            // No messages, release the permit and yield
            drop(permit);
            tokio::task::yield_now().await;
        }
    }

    async fn process_message(&self, msg: Message) -> Result<(), Error> {
        debug!("processing message {} {}", msg.to.unwrap(), msg.value.to_string());

        // First ensure the message is still valid
        if msg.is_expired() {
            return Err(Error::MessageExpired);
        }

        // Get the next unscheduled nonce and initial gas prices
        let nonce = self.nonce_manager.get_next_available_nonce().await;
        let (base_fee, priority_fee) = self.gas_manager.get_gas_price(msg.effective_priority()).await?;

        // Send transaction
        self.send_transaction(msg, nonce, base_fee, priority_fee, 0)
            .await?;
        Ok(())
    }

    async fn send_transaction(
        &self,
        msg: Message,
        nonce: u64,
        base_fee: u128,
        priority_fee: u128,
        replacement_count: u32,
    ) -> Result<(), Error> {
        Box::pin(async move {
            if replacement_count > MAX_REPLACEMENTS {
                self.nonce_manager.mark_nonce_available(nonce).await;
                return Err(Error::FeeIncreasesExceeded);
            }

            // Build and send the transaction
            let tx = TxEip1559 {
                chain_id: self.chain.id(),

                // Message fields
                to: msg.to.map_or(TxKind::Create, TxKind::Call),
                value: msg.value,
                input: msg.data.clone(),

                // Transaction wrapper fields
                nonce,
                gas_limit: msg.gas, // TODO: Sim for gas amount
                max_fee_per_gas: base_fee + priority_fee,
                max_priority_fee_per_gas: priority_fee,

                access_list: Default::default(),
            };
            let watcher = self.chain.send_transaction(tx).await?;
            let tx_hash = *watcher.tx_hash();

            // let stats = self.stats.get_mut();
            // stats.total_sent += 1;
            // if replacement_count > 0 {
            //     stats.total_replaced += 1;
            // }

            // Track the pending transaction
            self.pending.lock().await.insert(tx_hash, PendingTransaction {
                msg,
                nonce,
                priority_fee,
                replacement_count,
                created_at: Instant::now(),
            });

            // Watch the pending transaction for confirmation or timeout
            match self.watch_transaction(watcher).await {
                Ok(_) => {
                    match self.chain.get_receipt(tx_hash).await {
                        // We got a receipt
                        Ok(Some(receipt)) => self.handle_transaction_receipt(tx_hash, receipt).await,

                        // We timed out
                        Ok(None) => self.handle_transaction_dropped(tx_hash).await,

                        // Error getting the receipt
                        Err(e) => {
                            error!("error getting receipt: {}", e);
                            self.handle_transaction_dropped(tx_hash).await
                        }
                    };
                }
                // Transaction dropped
                Err(e) => {
                    error!("error building watcher: {}", e);
                    self.handle_transaction_dropped(tx_hash).await
                }
            };
            Ok(())
        }).await
    }

    async fn watch_transaction<'a>(&self, watcher: PendingTransactionBuilder<'a, PubSubFrontend, Ethereum>) -> Result<TxHash, Error> {
        // Configure the watcher
        let pending = watcher
            .with_required_confirmations(1)
            .with_timeout(Some(Duration::from_secs(TX_TIMEOUT)))
            .register().await?;

        // Wait for the watcher to confirm or timeout
        Ok(pending.await?)
    }

    async fn handle_transaction_receipt(
        &self,
        tx_hash: B256,
        receipt: TransactionReceipt,
    ) {
        // Handle reverts
        if !receipt.status() {
            // TODO: Handle better
            self.handle_transaction_dropped(tx_hash).await;
            return;
        }

        // Transaction confirmed; remove it from pending and update the gas and nonce trackers
        info!("Transaction {:?} confirmed", tx_hash);
        if let Some(pending_tx) = self.pending.lock().await.remove(&tx_hash) {
            let latency = pending_tx.created_at.elapsed();
            join(
                self.gas_manager.update_on_confirmation(latency, receipt.effective_gas_price),
                self.nonce_manager.update_current_nonce(pending_tx.nonce),
            ).await;
        }
    }

    async fn handle_transaction_dropped(&self, tx_hash: B256) {
        let mut pending = self.pending.lock().await;
        let mut pending_tx = match pending.remove(&tx_hash) {
            Some(tx) => tx,
            None => {
                println!(
                    "Transaction {} not found in in-flight pending map",
                    tx_hash
                );
                return;
            }
        };

        // If we have retries left then attempt to bump the the fee
        if pending_tx.msg.increment_retry() {
            if let Err(e) = self.bump_transaction_fee(tx_hash, pending_tx).await {
                println!("Failed to replace transaction: {:?}", e);
            }
            return;
        }

        // No retries left so log this and abandon
        // TODO: Make requeue to try again later?
        println!(
            "Message for transaction {:?} failed after max retries",
            tx_hash
        );
    }

    async fn bump_transaction_fee(
        &self,
        tx_hash: B256,
        pending_tx: PendingTransaction,
    ) -> Result<(), Error> {
        // Decide new fees
        let base_fee = self.gas_manager.get_base_fee().await;
        let new_priority = percent_change(pending_tx.priority_fee, GAS_PRICE_INCREASE_PERCENT);

        // Remove existing transaction
        self.pending.lock().await.remove(&tx_hash);

        // Send the same msg at the same nonce, with new fees and an incremented replacement_count
        self.send_transaction(
            pending_tx.msg,
            pending_tx.nonce,
            base_fee,
            new_priority,
            pending_tx.replacement_count + 1,
        )
            .await
    }
}

fn percent_change(n: u128, percent: u8) -> u128 {
    n * percent as u128 / 100
}
