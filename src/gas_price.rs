use std::collections::VecDeque;
use tokio::sync::Mutex;
use web3::types::U256;
use std::time::Duration;
use crate::Result;

const MAX_PRIORITY_FEE: U256 = U256([100_000_000_000, 0, 0, 0]); // 100 Gwei
const INITIAL_PRIORITY_FEE: U256 = U256([1_000_000_000, 0, 0, 0]); // 1 Gwei
const CONFIRMATION_TIME_WINDOW: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CongestionLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy)]
pub enum TransactionUrgency {
    Low,
    Medium,
    High,
}

pub struct GasPriceManager {
    confirmation_times: Mutex<VecDeque<Duration>>,
    priority_fee: Mutex<U256>,
}

impl GasPriceManager {
    pub fn new() -> Self {
        Self {
            confirmation_times: Mutex::new(VecDeque::with_capacity(CONFIRMATION_TIME_WINDOW)),
            priority_fee: Mutex::new(INITIAL_PRIORITY_FEE),
        }
    }

    pub async fn get_gas_price(&self, urgency: TransactionUrgency) -> Result<(U256, U256)> {
        let base_fee = self.estimate_base_fee().await;
        let priority_fee = self.calculate_priority_fee(urgency).await;
        Ok((base_fee, priority_fee))
    }

    async fn calculate_priority_fee(&self, urgency: TransactionUrgency) -> U256 {
        let base_priority_fee = *self.priority_fee.lock().await;
        let congestion = self.analyze_network_congestion().await;

        let congestion_multiplier = match congestion {
            CongestionLevel::Low => 1,
            CongestionLevel::Medium => 2,
            CongestionLevel::High => 3,
        };

        let urgency_multiplier = match urgency {
            TransactionUrgency::Low => 1,
            TransactionUrgency::Medium => 2,
            TransactionUrgency::High => 3,
        };

        let fee: U256 = base_priority_fee * congestion_multiplier * urgency_multiplier;
        fee.min(MAX_PRIORITY_FEE)
    }

    async fn analyze_network_congestion(&self) -> CongestionLevel {
        let confirmation_times = self.confirmation_times.lock().await;
        if confirmation_times.is_empty() {
            return CongestionLevel::Medium;
        }

        let avg_time = confirmation_times.iter().sum::<Duration>() / confirmation_times.len() as u32;

        if avg_time < Duration::from_secs(15) {
            CongestionLevel::Low
        } else if avg_time < Duration::from_secs(60) {
            CongestionLevel::Medium
        } else {
            CongestionLevel::High
        }
    }

    pub async fn update_on_confirmation(&self, confirmation_time: Duration, used_priority_fee: U256) {
        let mut confirmation_times = self.confirmation_times.lock().await;
        confirmation_times.push_back(confirmation_time);
        if confirmation_times.len() > CONFIRMATION_TIME_WINDOW {
            confirmation_times.pop_front();
        }
        drop(confirmation_times);

        let mut priority_fee = self.priority_fee.lock().await;
        *priority_fee = (*priority_fee + used_priority_fee) / 2;
    }

    async fn estimate_base_fee(&self) -> U256 {
        // Mock implementation - replace with actual base fee estimation logic
        U256::from(2_000_000_000) // 2 Gwei
    }
}

impl Default for GasPriceManager {
    fn default() -> Self {
        Self::new()
    }
}
