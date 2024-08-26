use crate::Error;
use std::{collections::VecDeque, time::Duration};
use tokio::sync::Mutex;
use crate::message::MAX_PRIORITY;

const MAX_PRIORITY_FEE: u128 = 100_000_000_000; // 100 Gwei
const INITIAL_PRIORITY_FEE: u128 = 1_000_000_000; // 1 Gwei
const INITIAL_BASE_FEE: u128 = 2_000_000_000; // 2 Gwei

const CONFIRMATION_TIME_WINDOW: usize = 10;
const CONGESTION_THRESHOLD_LOW: Duration = Duration::from_secs(15);
const CONGESTION_THRESHOLD_MEDIUM: Duration = Duration::from_secs(60);

/// Network congestion levels for gas price calculation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CongestionLevel {
    Low,
    Medium,
    High,
}

impl From<CongestionLevel> for u128 {
    fn from(level: CongestionLevel) -> u128 {
        match level {
            CongestionLevel::Low => 1,
            CongestionLevel::Medium => 2,
            CongestionLevel::High => 3,
        }
    }
}

/// Manages gas prices based on network congestion and user priority
pub(crate) struct GasPriceManager {
    confirmation_times: Mutex<VecDeque<Duration>>,
    priority_fee: Mutex<u128>,
}

impl GasPriceManager {
    /// Creates a new `GasPriceManager` with initial values
    pub fn new() -> Self {
        Self {
            confirmation_times: Mutex::new(VecDeque::with_capacity(CONFIRMATION_TIME_WINDOW)),
            priority_fee: Mutex::new(INITIAL_PRIORITY_FEE),
        }
    }

    /// Returns the estimated gas price based on network congestion and user priority
    pub async fn get_gas_price(&self, priority: u32) -> Result<(u128, u128), Error> {
        let base_fee = self.get_base_fee().await;
        let priority_fee = self.calculate_priority_fee(priority).await;
        Ok((base_fee, priority_fee))
    }

    /// Calculates the priority fee based on network congestion and user priority
    async fn calculate_priority_fee(&self, priority: u32) -> u128 {
        // Base priority is the minimum we want to use as a priority fee
        let base_priority_fee = *self.priority_fee.lock().await;

        // Get network congestion influence
        let congestion = self.analyze_network_congestion().await;
        let congestion_multiplier: u128 = congestion.into();

        // Get a priority multiplier from 100% to 200% based on the given priority
        let priority_multiplier: u128 = 100 + percent(priority.min(MAX_PRIORITY), MAX_PRIORITY);

        // Calculate the priority fee to use for this trans)action
        let fee: u128 = base_priority_fee * congestion_multiplier * priority_multiplier;
        let fee = fee / 100;
        fee.min(MAX_PRIORITY_FEE)
    }

    /// Analyzes the network congestion based on recent confirmation times
    async fn analyze_network_congestion(&self) -> CongestionLevel {
        let confirmation_times = self.confirmation_times.lock().await;
        if confirmation_times.is_empty() {
            return CongestionLevel::Medium;
        }

        let avg_time =
            confirmation_times.iter().sum::<Duration>() / confirmation_times.len() as u32;

        if avg_time < CONGESTION_THRESHOLD_LOW {
            CongestionLevel::Low
        } else if avg_time < CONGESTION_THRESHOLD_MEDIUM {
            CongestionLevel::Medium
        } else {
            CongestionLevel::High
        }
    }

    /// Updates the gas price manager based on the confirmation time and used priority fee
    pub async fn update_on_confirmation(
        &self,
        confirmation_time: Duration,
        used_priority_fee: u128,
    ) {
        let mut confirmation_times = self.confirmation_times.lock().await;
        confirmation_times.push_back(confirmation_time);
        if confirmation_times.len() > CONFIRMATION_TIME_WINDOW {
            confirmation_times.pop_front();
        }
        drop(confirmation_times);

        let mut priority_fee = self.priority_fee.lock().await;
        *priority_fee = (*priority_fee + used_priority_fee) / 2;
    }

    /// Returns the base fee of the next block
    /// Mock implementation - replace with actual base fee estimation logic
    pub(crate) async fn get_base_fee(&self) -> u128 {
        INITIAL_BASE_FEE
    }
}

impl Default for GasPriceManager {
    fn default() -> Self {
        Self::new()
    }
}

fn percent(x: u32, y: u32) -> u128 {
    ((x as f64 / y as f64) * 100.0) as u128
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::test;

    #[test]
    async fn test_initial_gas_price() {
        let manager = GasPriceManager::new();
        let (base_fee, priority_fee) = manager.get_gas_price(0).await.unwrap();
        assert_eq!(base_fee, INITIAL_BASE_FEE);

        // 2 Gwei (initial priority fee * initial congestion)
        assert_eq!(priority_fee, 2_000_000_000);
    }

    #[test]
    async fn test_priority_influence() {
        let manager = GasPriceManager::new();
        let (_, priority_fee_1) = manager.get_gas_price(1).await.unwrap();
        let (_, priority_fee_5) = manager.get_gas_price(5).await.unwrap();
        assert!(priority_fee_5 > priority_fee_1);
    }

    #[test]
    async fn test_max_priority_fee() {
        let manager = GasPriceManager::new();
        let (_, priority_fee) = manager.get_gas_price(MAX_PRIORITY).await.unwrap();
        assert!(priority_fee <= MAX_PRIORITY_FEE);

        let (_, priority_fee2) = manager.get_gas_price(MAX_PRIORITY + 100).await.unwrap();
        assert!(priority_fee == priority_fee2);
    }

    #[test]
    async fn test_congestion_levels() {
        let manager = GasPriceManager::new();

        // Test low congestion
        for _ in 0..10 {
            manager
                .update_on_confirmation(Duration::from_secs(10), 1_000_000_000)
                .await;
        }
        let (_, priority_fee_low) = manager.get_gas_price(1).await.unwrap();

        // Test medium congestion
        for _ in 0..10 {
            manager
                .update_on_confirmation(Duration::from_secs(30), 1_000_000_000)
                .await;
        }
        let (_, priority_fee_medium) = manager.get_gas_price(1).await.unwrap();

        // Test high congestion
        for _ in 0..10 {
            manager
                .update_on_confirmation(Duration::from_secs(70),1_000_000_000)
                .await;
        }
        let (_, priority_fee_high) = manager.get_gas_price(1).await.unwrap();

        assert!(priority_fee_low < priority_fee_medium);
        assert!(priority_fee_medium < priority_fee_high);
    }

    #[test]
    async fn test_update_on_confirmation() {
        let manager = GasPriceManager::new();
        let initial_priority_fee = *manager.priority_fee.lock().await;

        manager
            .update_on_confirmation(Duration::from_secs(30), 2_000_000_000)
            .await;

        let updated_priority_fee = *manager.priority_fee.lock().await;
        assert!(updated_priority_fee > initial_priority_fee);
    }

    #[test]
    async fn test_confirmation_time_window() {
        let manager = GasPriceManager::new();

        for i in 0..=CONFIRMATION_TIME_WINDOW {
            manager
                .update_on_confirmation(Duration::from_secs(i as u64), 1_000_000_000)
                .await;
        }

        let confirmation_times = manager.confirmation_times.lock().await;
        assert_eq!(confirmation_times.len(), CONFIRMATION_TIME_WINDOW);
        assert_eq!(confirmation_times.front(), Some(&Duration::from_secs(1)));
        assert_eq!(
            confirmation_times.back(),
            Some(&Duration::from_secs(CONFIRMATION_TIME_WINDOW as u64))
        );
    }
}
