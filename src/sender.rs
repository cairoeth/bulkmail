use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use web3::types::{TransactionReceipt, TransactionRequest, H256, U256, U64};
use web3::{Transport, Web3};
use crate::{Message, PriorityQueue, GasPriceManager, Error, BLOCK_TIME};

const MAX_IN_FLIGHT_TRANSACTIONS: usize = 16;
const MAX_REPLACEMENTS: u32 = 3;
const REPLACEMENT_INTERVAL: Duration = Duration::from_secs((BLOCK_TIME * 5) as u64);

type PendingMap = HashMap<H256, PendingTransaction>;
type SharedPendingMap = Arc<Mutex<PendingMap>>;
type SharedQueue = Arc<Mutex<PriorityQueue>>;

#[derive(Clone)]
struct PendingTransaction {
    message: Message,
    created_at: Instant,
    replacement_count: u32,
    last_replacement_time: Instant,
    current_gas_price: U256,
    nonce: U256,
}

#[derive(Clone)]
pub struct Sender<T: Transport + Send + 'static> {
    web3: Web3<T>,
    queue: SharedQueue,
    pending: SharedPendingMap,
    nonce: Arc<Mutex<U256>>,
    gas_price_manager: Arc<GasPriceManager>,
    max_in_flight: Arc<Semaphore>,
}

impl<T: Transport + Send + Sync> Sender<T> {
    pub async fn new(transport: T) -> Result<Self, Error> {
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

    pub async fn add_message(&self, message: Message) -> Result<(), Error> {
        self.queue.lock().await.push(message);
        Ok(())
    }

    pub async fn run(&self) -> Result<(), Error> where <T as Transport>::Out: Send {
        let mut interval = tokio::time::interval(REPLACEMENT_INTERVAL);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.check_stuck_transactions().await;
                }
                _ = self.process_next_message() => {}
            }
        }
    }

    async fn process_next_message(&self) where <T as Transport>::Out: Send {
        let permit = self.max_in_flight.clone().acquire_owned().await.unwrap();

        if let Some(msg) = self.queue.lock().await.pop() {
            if msg.is_expired() {
                eprintln!("Discarding expired message");
                return;
            }

            let sender = self.clone();
            tokio::spawn(async move {
                if let Err(e) = sender.process_message(msg).await {
                    eprintln!("Error processing message: {:?}", e);
                }
                // The permit is dropped here, releasing it back to the semaphore
            });
        } else {
            drop(permit);
            tokio::task::yield_now().await;
        }
    }

    async fn process_message(&self, mut msg: Message) -> Result<(), Error> where <T as Transport>::Out: Send {
        // Check deadline
        if msg.is_expired() {
            return Err(Error::MessageExpired);
        }

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
        let gas_price = base_fee + priority_fee;
        self.pending.lock().await.insert(tx_hash, PendingTransaction {
            message: msg,
            created_at: Instant::now(),
            replacement_count: 0,
            last_replacement_time: Instant::now(),
            current_gas_price: gas_price,
            nonce,
        });

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

    async fn check_stuck_transactions(&self) {
        let now = Instant::now();
        let transactions_to_replace = {
            let pending = self.pending.lock().await;
            pending
                .iter()
                .filter_map(|(tx_hash, info)| {
                    if now.duration_since(info.last_replacement_time) > REPLACEMENT_INTERVAL
                        && info.replacement_count < MAX_REPLACEMENTS
                    {
                        Some((*tx_hash, info.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        for (tx_hash, info) in transactions_to_replace {
            if let Err(e) = self.replace_transaction(tx_hash, info).await {
                eprintln!("Failed to replace transaction: {:?}", e);
            }
        }
    }

    async fn replace_transaction(&self, tx_hash: H256, mut pending_tx: PendingTransaction) -> Result<(), Error> {
        let base_fee = self.gas_price_manager.get_base_fee().await;
        let current_gas_price = pending_tx.current_gas_price.max(base_fee);
        let new_gas_price = current_gas_price * 110 / 100;
        let new_priority_fee = new_gas_price - base_fee;

        // Create a new transaction with the same details but higher gas price
        let msg = &pending_tx.message;
        let new_tx = TransactionRequest {
            from: msg.from,
            to: msg.to,
            value: msg.value,
            gas: Some(msg.gas),
            data: msg.data.clone(),

            nonce: Some(pending_tx.nonce),
            max_fee_per_gas: Some(new_gas_price),
            max_priority_fee_per_gas: Some(new_priority_fee),

            gas_price: None,
            transaction_type: Some(U64::from(2)),
            ..Default::default()
        };

        // Send the new transaction
        let new_hash = self.web3.eth().send_transaction(new_tx).await?;

        // Update the pending transaction
        pending_tx.replacement_count += 1;
        pending_tx.last_replacement_time = Instant::now();
        pending_tx.current_gas_price = new_gas_price;

        // Update the pending transactions map
        let mut pending = self.pending.lock().await;
        pending.remove(&tx_hash);
        pending.insert(new_hash, pending_tx);

        Ok(())
    }
}

async fn wait_for_confirmation<T: Transport>(
    eth: web3::api::Eth<T>,
    pending: SharedPendingMap,
    gas_price_manager: Arc<GasPriceManager>,
    queue: SharedQueue,
    tx_hash: H256
) where
    T: Send + Sync + 'static,
{
    match eth.transaction_receipt(tx_hash).await    {
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
    pending: SharedPendingMap,
    gas_price_manager: Arc<GasPriceManager>,
    queue: SharedQueue,
    tx_hash: H256,
    receipt: TransactionReceipt
) {
    if receipt.status == Some(1.into()) {
        println!("Transaction {:?} confirmed", tx_hash);
        if let Some(pending_tx) = pending.lock().await.remove(&tx_hash) {
            let confirmation_time = pending_tx.created_at.elapsed();
            gas_price_manager.update_on_confirmation(confirmation_time, receipt.effective_gas_price.unwrap_or_default()).await;
        }
    } else {
        eprintln!("Transaction {:?} failed", tx_hash);
        handle_failed_transaction(pending, queue, tx_hash).await;
    }
}

async fn handle_failed_transaction(
    pending: SharedPendingMap,
    queue: SharedQueue,
    tx_hash: H256
) {
    let mut pending = pending.lock().await;
    if let Some(mut pending_tx) = pending.remove(&tx_hash) {
        pending_tx.message.increment_retry();
        if pending_tx.message.can_retry() {
            // Increase priority and push back to queue
            pending_tx.message.priority = pending_tx.message.priority.saturating_add(1);
            drop(pending);
            queue.lock().await.push(pending_tx.message);
        } else {
            eprintln!("Message for transaction {:?} failed after max retries", tx_hash);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use super::*;
    use mockall::predicate::*;
    use serde_json::Value;
    use web3::types::{H256, U256, Address};
    use web3::futures::Future;
    use std::pin::Pin;
    use jsonrpc_core::{Call, Id, MethodCall, Params, Version};

    #[derive(Clone, Debug)]
    struct MockTransport {
        send_responses: Arc<Mutex<VecDeque<Result<Value, web3::Error>>>>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                send_responses: Arc::new(Mutex::new(VecDeque::new())),
            }
        }

        async fn add_response(&self, response: Result<Value, web3::Error>) {
            self.send_responses.lock().await.push_back(response);
        }
    }

    impl Transport for MockTransport {
        type Out = Pin<Box<dyn Future<Output = Result<Value, web3::Error>> + Send>>;

        fn prepare(&self, _method: &str, _params: Vec<Value>) -> (web3::RequestId, Call) {
            let m = MethodCall {
                jsonrpc: Some(Version::V2),
                method: "eth_sendTransaction".to_string(),
                params: Params::Array(vec![]),
                id: Id::Num(1),
            };
            (0, Call::MethodCall(m))
        }

        fn send(&self, _id: web3::RequestId, _request: Call) -> Self::Out {
            let send_responses = self.send_responses.clone();
            Box::pin(async move {
                let a = send_responses.lock().await.pop_front().unwrap_or(Err(web3::Error::Internal));
                a
            })
        }
    }

    // Helper function to create a test message
    fn create_test_message() -> Message {
        Message {
            from: Address::zero(),
            to: Some(Address::zero()),
            value: Some(U256::from(1000)),
            data: None,
            gas: U256::from(21_000),
            priority: 1,
            dependencies: vec![],
            deadline: None,
            created_at: Instant::now(),
            retry_count: 0,
        }
    }

    #[tokio::test]
    async fn test_add_message() {
        let mock_transport = MockTransport::new();
        let sender = Sender::new(mock_transport).await.unwrap();
        let message = create_test_message();

        assert!(sender.add_message(message.clone()).await.is_ok());

        let queue = sender.queue.lock().await;
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn test_process_message() {
        let mock_transport = MockTransport::new();
        mock_transport.add_response(Ok(Value::String("0x1111111111111111111111111111111111111111111111111111111111111111".to_string()))).await;
        mock_transport.add_response(Ok(Value::String("0x2222222222222222222222222222222222222222222222222222222222222222".to_string()))).await;
        let sender = Sender::new(mock_transport).await.unwrap();

        let message = create_test_message();

        assert!(sender.process_message(message).await.is_ok());

        let pending = sender.pending.lock().await;
        assert_eq!(pending.len(), 1);
    }

    #[tokio::test]
    async fn test_check_stuck_transactions() {
        // Create sender with mock client
        let mock_transport = MockTransport::new();
        mock_transport.add_response(Ok(Value::String("0x1111111111111111111111111111111111111111111111111111111111111111".to_string()))).await; // first send_transaction
        mock_transport.add_response(Ok(Value::String("0x2222222222222222222222222222222222222222222222222222222222222222".to_string()))).await; // replacement send_transaction
        let sender = Sender::new(mock_transport).await.unwrap();

        // Add a stuck transaction
        {
            let tx_hash = H256::random();
            sender.pending.lock().await.insert(tx_hash,  PendingTransaction {
                message: create_test_message(),
                created_at: Instant::now(),
                replacement_count: 0,
                last_replacement_time: Instant::now() - Duration::from_secs(REPLACEMENT_INTERVAL.as_secs() + 1),
                current_gas_price: U256::from(1_000_000_000),
                nonce: U256::zero(),
            });
        }

        // Run check_stuck_transactions
        sender.check_stuck_transactions().await;

        // Verify that the transaction was replaced
        let pending = sender.pending.lock().await;
        assert_eq!(pending.len(), 1);
        let replaced_tx = pending.values().next().unwrap();
        assert_eq!(replaced_tx.replacement_count, 1);
        assert!(replaced_tx.current_gas_price > U256::from(1_000_000_000));
    }
}
