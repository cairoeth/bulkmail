use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Semaphore};
use web3::types::{TransactionReceipt, TransactionRequest, H256, U256, U64};
use web3::{Transport, Web3};
use crate::{Result, Message, PriorityQueue, GasPriceManager};

const MAX_IN_FLIGHT_TRANSACTIONS: usize = 16;

#[derive(Clone)]
pub struct Sender<T: Transport + Send + 'static> {
    web3: Web3<T>,
    queue: Arc<Mutex<PriorityQueue>>,
    pending: Arc<Mutex<HashMap<H256, (Message, Instant)>>>,
    nonce: Arc<Mutex<U256>>,
    gas_price_manager: Arc<GasPriceManager>,
    max_in_flight: Arc<Semaphore>,
}

impl<T: Transport + Send + Sync> Sender<T> {
    pub async fn new(transport: T) -> Result<Self> {
        let web3 = Web3::new(transport);
        Ok(Self {
            web3,
            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            nonce: Arc::new(Mutex::new(U256::zero())), // We'll set this properly when sending
            gas_price_manager: Arc::new(GasPriceManager::new()),
            max_in_flight: Arc::new(Semaphore::new(MAX_IN_FLIGHT_TRANSACTIONS)),
        })
    }

    pub async fn add_message(&self, message: Message) -> Result<()> {
        self.queue.lock().await.push(message);
        Ok(())
    }

    pub async fn run(&self) -> Result<()> where <T as Transport>::Out: Send {
        loop {
            // Acquire a permit before processing a message
            let _permit = self.max_in_flight.clone().acquire_owned().await.unwrap();

            // Try to get a message from the queue
            if let Some(msg) = self.queue.lock().await.pop() {
                // Process the message directly
                if let Err(e) = self.process_message(msg).await {
                    eprintln!("Error processing message: {:?}", e);
                }
                // The permit is dropped here, releasing it back to the semaphore
                continue;
            }

            // If there are no messages, release the permit and yield to the scheduler
            drop(_permit);
            tokio::task::yield_now().await;
        }
    }

    async fn process_message(&self, mut msg: Message) -> Result<()> where <T as Transport>::Out: Send {
        // Check dependencies
        {
            let pending = self.pending.lock().await;
            for dep in &msg.dependencies {
                if pending.contains_key(dep) {
                    // Dependency not yet confirmed, push back to queue
                    drop(pending);
                    self.queue.lock().await.push(msg);
                    return Ok(());
                }
            }
        }

        // Set nonce
        let nonce = {
            let mut nonce = self.nonce.lock().await;
            *nonce = self.web3.eth().transaction_count(msg.from, None).await?;
            let current_nonce = *nonce;
            *nonce += 1.into();
            current_nonce
        };

        // Set gas price
        let (base_fee, priority_fee) = self.gas_price_manager.get_gas_price(msg.priority).await?;

        // Create TransactionRequest
        let tx_request = TransactionRequest {
            // Message data
            from: msg.from,
            to: msg.to,
            gas: Some(msg.gas),
            value: msg.value,
            data: msg.data.clone().map(|data| data.clone()),

            // Late-bound transaction params
            nonce: Some(nonce),
            max_fee_per_gas: Some(base_fee + priority_fee),
            max_priority_fee_per_gas: Some(priority_fee),

            // Static transaction params
            gas_price: None,
            condition: None,
            access_list: None,
            transaction_type: Some(U64::from(2)),
        };

        // Send transaction
        let tx_hash = match self.web3.eth().send_transaction(tx_request).await {
            Ok(hash) => hash,
            Err(e) => {
                // Handle error (e.g., low gas price)
                msg.increment_retry();
                if msg.can_retry() {
                    self.queue.lock().await.push(msg);
                } else {
                    eprintln!("Message failed after max retries: {:?}", e);
                }
                return Ok(());
            }
        };

        // Add to pending
        self.pending.lock().await.insert(tx_hash, (msg, Instant::now()));

        // Spawn a new task to wait for confirmation
        let pending = self.pending.clone();
        let gas_price_manager = self.gas_price_manager.clone();
        let queue = self.queue.clone();
        let eth = self.web3.eth().clone();

        tokio::spawn(async move {
            wait_for_confirmation(eth, pending, gas_price_manager, queue, tx_hash).await;
        });


        Ok(())
    }
}

async fn wait_for_confirmation<T: Transport>(
    eth: web3::api::Eth<T>,
    pending: Arc<Mutex<HashMap<H256, (Message, Instant)>>>,
    gas_price_manager: Arc<GasPriceManager>,
    queue: Arc<Mutex<PriorityQueue>>,
    tx_hash: H256
) where
    T: Send + Sync + 'static,
{
    match eth.transaction_receipt(tx_hash).await {
        Ok(Some(receipt)) => {
            handle_transaction_receipt(pending, gas_price_manager, queue, tx_hash, receipt).await;
        }
        Ok(None) => {
            eprintln!("Transaction {:?} receipt not found", tx_hash);
            handle_failed_transaction(pending, queue, tx_hash).await;
        }
        Err(e) => {
            eprintln!("Error waiting for transaction {:?}: {:?}", tx_hash, e);
            handle_failed_transaction(pending, queue, tx_hash).await;
        }
    }
}

async fn handle_transaction_receipt(
    pending: Arc<Mutex<HashMap<H256, (Message, Instant)>>>,
    gas_price_manager: Arc<GasPriceManager>,
    queue: Arc<Mutex<PriorityQueue>>,
    tx_hash: H256,
    receipt: TransactionReceipt
) {
    if receipt.status == Some(1.into()) {
        println!("Transaction {:?} confirmed", tx_hash);
        if let Some((_, start_time)) = pending.lock().await.remove(&tx_hash) {
            let confirmation_time = start_time.elapsed();
            gas_price_manager.update_on_confirmation(confirmation_time, receipt.effective_gas_price.unwrap_or_default()).await;
        }
    } else {
        eprintln!("Transaction {:?} failed", tx_hash);
        handle_failed_transaction(pending, queue, tx_hash).await;
    }
}

async fn handle_failed_transaction(
    pending: Arc<Mutex<HashMap<H256, (Message, Instant)>>>,
    queue: Arc<Mutex<PriorityQueue>>,
    tx_hash: H256
) {
    let mut pending = pending.lock().await;
    if let Some((mut msg, _)) = pending.remove(&tx_hash) {
        msg.increment_retry();
        if msg.can_retry() {
            // Increase priority and push back to queue
            msg.priority = msg.priority.saturating_add(1);
            drop(pending);
            queue.lock().await.push(msg);
        } else {
            eprintln!("Message {:?} failed after max retries", tx_hash);
        }
    }
}
