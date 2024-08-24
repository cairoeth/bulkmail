use crate::{Error, GasPriceManager, Message, PriorityQueue, BLOCK_TIME};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore};
use web3::{
    types::{CallRequest, TransactionReceipt, TransactionRequest, H256, U256, U64},
    Transport, Web3,
};

const MAX_IN_FLIGHT_TRANSACTIONS: usize = 16;
const MAX_REPLACEMENTS: u32 = 3;
const REPLACEMENT_INTERVAL: Duration = Duration::from_secs((BLOCK_TIME * 5) as u64);
const SIMULATION_GAS_LIMIT: u32 = 1_000_000;

type PendingMap = HashMap<H256, PendingTransaction>;
type SharedPendingMap = Arc<Mutex<PendingMap>>;
type SharedQueue = Arc<Mutex<PriorityQueue>>;

#[derive(Clone)]
struct PendingTransaction {
    msg: Message,
    created_at: Instant,
    replacement_count: u32,
    gas_price: U256,
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

    pub async fn run(&self) -> Result<(), Error>
    where
        <T as Transport>::Out: Send,
    {
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

    async fn process_next_message(&self)
    where
        <T as Transport>::Out: Send,
    {
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

    async fn process_message(&self, msg: Message) -> Result<(), Error>
    where
        <T as Transport>::Out: Send,
    {
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

        // Determine next nonce
        let nonce = {
            let mut nonce = self.nonce.lock().await;
            *nonce = self.web3.eth().transaction_count(msg.from, None).await?;
            let current_nonce = *nonce;
            *nonce += 1.into();
            current_nonce
        };

        // Get current initial gas price
        let (base_fee, priority_fee) = self.gas_price_manager.get_gas_price(msg.priority).await?;

        // Send transaction
        self.send_transaction(msg, nonce, base_fee, priority_fee, 0).await
    }

    async fn simulate_transaction(&self, tx: &TransactionRequest) -> Result<U256, Error> {
        Ok(self.web3.eth().estimate_gas(CallRequest {
            from: Some(tx.from),
            to: tx.to,
            value: tx.value,
            data: tx.data.clone(),

            gas: Some(U256::from(SIMULATION_GAS_LIMIT)),
            gas_price: tx.gas_price,
            max_fee_per_gas: tx.max_fee_per_gas,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,

            transaction_type: tx.transaction_type,
            ..Default::default()
        }, None).await?)
    }

    async fn send_transaction(
        &self,
        mut msg: Message,
        nonce: U256,
        base_fee: U256,
        priority_fee: U256,
        replacement_count: u32,
    ) -> Result<(), Error>
    where
        <T as Transport>::Out: Send,
    {
        // Create TransactionRequest
        let tx_request = self.build_transaction_request(&msg, nonce, base_fee, priority_fee).await?;

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
                return Err(Error::Web3Error(e));
            }
        };

        // Add to pending
        let gas_price = base_fee + priority_fee;
        let pending_tx = PendingTransaction {
            msg,
            nonce,
            gas_price,
            replacement_count,
            created_at: Instant::now(),
        };

        self.pending.lock().await.insert(tx_hash, pending_tx.clone());

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

    async fn check_stuck_transactions(&self)
    where
        <T as Transport>::Out: Send,
    {
        let now = Instant::now();
        let transactions_to_replace = {
            let pending = self.pending.lock().await;
            pending
                .iter()
                .filter_map(|(tx_hash, info)| {
                    if now.duration_since(info.created_at) > REPLACEMENT_INTERVAL
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

    async fn replace_transaction(
        &self,
        tx_hash: H256,
        pending_tx: PendingTransaction,
    ) -> Result<(), Error> where
        <T as Transport>::Out: Send,
    {
        let base_fee = self.gas_price_manager.get_base_fee().await;
        let current_gas_price = pending_tx.gas_price.max(base_fee);
        let new_gas_price = percent_change(current_gas_price, 110);
        let new_priority_fee = new_gas_price - base_fee;

        // Remove the old transaction from pending
        self.pending.lock().await.remove(&tx_hash);

        // Send the new transaction
        self.send_transaction(
            pending_tx.msg,
            pending_tx.nonce,
            base_fee,
            new_priority_fee,
            pending_tx.replacement_count + 1,
        )
            .await
    }

    async fn build_transaction_request(
        &self,
        msg: &Message,
        nonce: U256,
        base_fee: U256,
        priority_fee: U256,
    ) -> Result<TransactionRequest, Error> {
        let mut tx = TransactionRequest {
            from: msg.from,
            to: msg.to,
            value: msg.value,
            data: msg.data.clone(),

            gas: None, // We'll set this below
            nonce: Some(nonce),
            max_fee_per_gas: Some(base_fee + priority_fee),
            max_priority_fee_per_gas: Some(priority_fee),

            transaction_type: Some(U64::from(2)),
            ..Default::default()
        };

        tx.gas = Some(match msg.gas {
            Some(g) => g,
            None => {
                // Simulate transaction to get gas used
                let gas = self.simulate_transaction(&tx).await?;
                percent_change(gas, 120)
            }
        });

        Ok(tx)
    }
}

fn percent_change(n: U256, percent: u8) -> U256 {
    n * U256::from(percent) / U256::from(100)
}


async fn wait_for_confirmation<T>(
    eth: web3::api::Eth<T>,
    pending: SharedPendingMap,
    gas_price_manager: Arc<GasPriceManager>,
    queue: SharedQueue,
    tx_hash: H256,
) where
    T: Transport + Send + Sync + 'static,
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
    pending: SharedPendingMap,
    gas_price_manager: Arc<GasPriceManager>,
    queue: SharedQueue,
    tx_hash: H256,
    receipt: TransactionReceipt,
) {
    if receipt.status == Some(1.into()) {
        println!("Transaction {:?} confirmed", tx_hash);
        if let Some(pending_tx) = pending.lock().await.remove(&tx_hash) {
            let confirmation_time = pending_tx.created_at.elapsed();
            gas_price_manager
                .update_on_confirmation(
                    confirmation_time,
                    receipt.effective_gas_price.unwrap_or_default(),
                )
                .await;
        }
    } else {
        eprintln!("Transaction {:?} failed", tx_hash);
        handle_failed_transaction(pending, queue, tx_hash).await;
    }
}

async fn handle_failed_transaction(pending: SharedPendingMap, queue: SharedQueue, tx_hash: H256) {
    let mut pending = pending.lock().await;
    if let Some(mut pending_tx) = pending.remove(&tx_hash) {
        pending_tx.msg.increment_retry();
        if pending_tx.msg.can_retry() {
            // Increase priority and push back to queue
            pending_tx.msg.priority = pending_tx.msg.priority.saturating_add(1);
            drop(pending);
            queue.lock().await.push(pending_tx.msg);
        } else {
            eprintln!(
                "Message for transaction {:?} failed after max retries",
                tx_hash
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonrpc_core::{Call, Id, MethodCall, Params, Version};
    use mockall::predicate::*;
    use serde_json::Value;
    use std::{collections::VecDeque, pin::Pin};
    use web3::{
        futures::Future,
        types::{Address, H256, U256},
    };

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
        type Out = Pin<Box<dyn Future<Output=Result<Value, web3::Error>> + Send>>;

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
                let a = send_responses
                    .lock()
                    .await
                    .pop_front()
                    .unwrap_or(Err(web3::Error::Internal));
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
            gas: Some(U256::from(21_000)),
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
        mock_transport
            .add_response(Ok(Value::String(
                "0x1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            )))
            .await;
        mock_transport
            .add_response(Ok(Value::String(
                "0x2222222222222222222222222222222222222222222222222222222222222222".to_string(),
            )))
            .await;
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
        mock_transport
            .add_response(Ok(Value::String(
                "0x1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            )))
            .await; // first send_transaction
        mock_transport
            .add_response(Ok(Value::String(
                "0x2222222222222222222222222222222222222222222222222222222222222222".to_string(),
            )))
            .await; // replacement send_transaction
        let sender = Sender::new(mock_transport).await.unwrap();

        // Add a stuck transaction
        {
            let tx_hash = H256::random();
            sender.pending.lock().await.insert(
                tx_hash,
                PendingTransaction {
                    msg: create_test_message(),
                    created_at: Instant::now(),
                    replacement_count: 0,
                    last_replacement_time: Instant::now()
                        - Duration::from_secs(REPLACEMENT_INTERVAL.as_secs() + 1),
                    gas_price: U256::from(1_000_000_000),
                    nonce: U256::zero(),
                },
            );
        }

        // Run check_stuck_transactions
        sender.check_stuck_transactions().await;

        // Verify that the transaction was replaced
        let pending = sender.pending.lock().await;
        assert_eq!(pending.len(), 1);
        let replaced_tx = pending.values().next().unwrap();
        assert_eq!(replaced_tx.replacement_count, 1);
        assert!(replaced_tx.gas_price > U256::from(1_000_000_000));
    }
}
