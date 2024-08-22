use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use web3::types::{Address, TransactionReceipt, TransactionRequest, H256, U256};
use web3::{Transport, Web3};
use crate::{Result, Transaction, PriorityQueue, GasPriceManager};
use crate::gas_price::TransactionUrgency;

pub struct Sender<T: Transport> {
    web3: Web3<T>,
    queue: Arc<Mutex<PriorityQueue>>,
    pending: Arc<Mutex<HashMap<H256, (Transaction, Instant)>>>,
    nonce: Arc<Mutex<U256>>,
    gas_price_manager: Arc<GasPriceManager>,
    from_address: Address,
}

impl<T: Transport> Sender<T> {
    pub async fn new(transport: T, from_address: Address) -> Result<Self> {
        let web3 = Web3::new(transport);
        let nonce = web3.eth().transaction_count(from_address, None).await?;
        Ok(Self {
            web3,
            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            nonce: Arc::new(Mutex::new(nonce)),
            gas_price_manager: Arc::new(GasPriceManager::new()),
            from_address,
        })
    }

    pub async fn add_transaction(&self, transaction: Transaction) -> Result<()> {
        self.queue.lock().await.push(transaction);
        Ok(())
    }

    pub async fn run(&self) -> Result<()> {
        loop {
            if let Some(tx) = self.queue.lock().await.pop() {
                if let Err(e) = self.process_transaction(tx).await {
                    eprintln!("Error processing transaction: {:?}", e);
                }
            } else {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    async fn process_transaction(&self, mut tx: Transaction) -> Result<()> {
        // Check dependencies
        {
            let pending = self.pending.lock().await;
            for dep in &tx.dependencies {
                if pending.contains_key(dep) {
                    // Dependency not yet confirmed, push back to queue
                    drop(pending);
                    self.queue.lock().await.push(tx);
                    return Ok(());
                }
            }
        }

        // Set nonce
        let nonce = {
            let mut nonce = self.nonce.lock().await;
            let current_nonce = *nonce;
            *nonce += 1.into();
            current_nonce
        };
        tx.tx.nonce = Some(nonce);

        // Set gas price
        let (base_fee, priority_fee) = self.gas_price_manager.get_gas_price(tx.urgency).await?;
        tx.tx.max_fee_per_gas = Some(base_fee + priority_fee);
        tx.tx.max_priority_fee_per_gas = Some(priority_fee);

        // Convert TransactionParameters to TransactionRequest
        let tx_request = TransactionRequest {
            from: self.from_address,
            to: tx.tx.to,
            gas: Some(tx.tx.gas),
            gas_price: None,
            value: None,
            data: Some(tx.tx.data.clone()),
            nonce: tx.tx.nonce,
            condition: None,
            transaction_type: tx.tx.transaction_type,
            access_list: tx.tx.access_list.clone(),
            max_fee_per_gas: tx.tx.max_fee_per_gas,
            max_priority_fee_per_gas: tx.tx.max_priority_fee_per_gas,
        };

        // Send transaction
        let tx_hash = match self.web3.eth().send_transaction(tx_request).await {
            Ok(hash) => hash,
            Err(e) => {
                // Handle error (e.g., low gas price)
                tx.increment_retry();
                if tx.can_retry() {
                    // Push back to queue with increased urgency
                    tx.urgency = match tx.urgency {
                        TransactionUrgency::Low => TransactionUrgency::Medium,
                        TransactionUrgency::Medium => TransactionUrgency::High,
                        TransactionUrgency::High => TransactionUrgency::High,
                    };
                    self.queue.lock().await.push(tx);
                } else {
                    eprintln!("Transaction failed after max retries: {:?}", e);
                }
                return Ok(());
            }
        };

        // Add to pending
        self.pending.lock().await.insert(tx_hash, (tx, Instant::now()));

        // Wait for confirmation
        self.wait_for_confirmation(tx_hash).await;

        Ok(())
    }

    async fn wait_for_confirmation(&self, tx_hash: H256) {
        match self.web3.eth().transaction_receipt(tx_hash).await {
            Ok(receipt) => {
                let receipt = match receipt {
                    Some(receipt) => receipt,
                    None => {
                        eprintln!("Transaction {:?} receipt not found", tx_hash);
                        return;
                    }
                };
                self.handle_transaction_receipt(tx_hash, receipt).await;
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
            if let Some((tx, start_time)) = self.pending.lock().await.remove(&tx_hash) {
                let confirmation_time = start_time.elapsed();
                self.gas_price_manager.update_on_confirmation(confirmation_time, tx.tx.max_priority_fee_per_gas.unwrap()).await;
            }
        } else {
            eprintln!("Transaction {:?} failed", tx_hash);
            self.handle_failed_transaction(tx_hash).await;
        }
    }

    async fn handle_failed_transaction(&self, tx_hash: H256) {
        let mut pending = self.pending.lock().await;
        if let Some((mut tx, _)) = pending.remove(&tx_hash) {
            tx.increment_retry();
            if tx.can_retry() {
                // Increase urgency and push back to queue
                tx.urgency = match tx.urgency {
                    TransactionUrgency::Low => TransactionUrgency::Medium,
                    TransactionUrgency::Medium => TransactionUrgency::High,
                    TransactionUrgency::High => TransactionUrgency::High,
                };
                drop(pending);
                self.queue.lock().await.push(tx);
            } else {
                eprintln!("Transaction {:?} failed after max retries", tx_hash);
            }
        }
    }
}
