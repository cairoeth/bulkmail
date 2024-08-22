use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use web3::types::{TransactionReceipt, TransactionRequest, H256, U256, U64};
use web3::{Transport, Web3};
use crate::{Result, Message, PriorityQueue, GasPriceManager};

pub struct Sender<T: Transport> {
    web3: Web3<T>,
    queue: Arc<Mutex<PriorityQueue>>,
    pending: Arc<Mutex<HashMap<H256, (Message, Instant)>>>,
    nonce: Arc<Mutex<U256>>,
    gas_price_manager: Arc<GasPriceManager>,
}

impl<T: Transport> Sender<T> {
    pub async fn new(transport: T) -> Result<Self> {
        let web3 = Web3::new(transport);
        Ok(Self {
            web3,
            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            nonce: Arc::new(Mutex::new(U256::zero())), // We'll set this properly when sending
            gas_price_manager: Arc::new(GasPriceManager::new()),
        })
    }

    pub async fn add_message(&self, message: Message) -> Result<()> {
        self.queue.lock().await.push(message);
        Ok(())
    }

    pub async fn run(&self) -> Result<()> {
        loop {
            if let Some(msg) = self.queue.lock().await.pop() {
                if let Err(e) = self.process_message(msg).await {
                    eprintln!("Error processing message: {:?}", e);
                }
            } else {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    async fn process_message(&self, mut msg: Message) -> Result<()> {
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

        // Wait for confirmation
        self.wait_for_confirmation(tx_hash).await;

        Ok(())
    }

    async fn wait_for_confirmation(&self, tx_hash: H256) {
        match self.web3.eth().transaction_receipt(tx_hash).await {
            Ok(Some(receipt)) => {
                self.handle_transaction_receipt(tx_hash, receipt).await;
            }
            Ok(None) => {
                eprintln!("Transaction {:?} receipt not found", tx_hash);
            }
            Err(e) => {
                eprintln!("Error waiting for transaction {:?}: {:?}", tx_hash, e);
                self.handle_failed_transaction(tx_hash).await;
            }
        }
    }

    async fn handle_transaction_receipt(&self, tx_hash: H256, receipt: TransactionReceipt) {
        if receipt.status == Some(1.into()) {
            println!("Transaction {:?} confirmed", tx_hash);
            if let Some((_, start_time)) = self.pending.lock().await.remove(&tx_hash) {
                let confirmation_time = start_time.elapsed();
                self.gas_price_manager.update_on_confirmation(confirmation_time, receipt.effective_gas_price.unwrap_or_default()).await;
            }
        } else {
            eprintln!("Transaction {:?} failed", tx_hash);
            self.handle_failed_transaction(tx_hash).await;
        }
    }

    async fn handle_failed_transaction(&self, tx_hash: H256) {
        let mut pending = self.pending.lock().await;
        if let Some((mut msg, _)) = pending.remove(&tx_hash) {
            msg.increment_retry();
            if msg.can_retry() {
                drop(pending);
                self.queue.lock().await.push(msg);
            } else {
                eprintln!("Message {:?} failed after max retries", tx_hash);
            }
        }
    }
}
