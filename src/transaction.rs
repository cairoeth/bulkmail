use std::time::Instant;
use web3::types::{TransactionParameters, H256};
use crate::{Result, HappyChainError};
use crate::gas_price::TransactionUrgency;

pub const MAX_DEPENDENCIES: usize = 5;
pub const MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone)]
pub struct Transaction {
    pub tx: TransactionParameters,
    pub priority: u32,
    pub urgency: TransactionUrgency,
    pub dependencies: Vec<H256>,
    pub created_at: Instant,
    pub retry_count: u32,
}

impl Transaction {
    pub fn new(tx: TransactionParameters, priority: u32, urgency: TransactionUrgency, dependencies: Vec<H256>) -> Result<Self> {
        if dependencies.len() > MAX_DEPENDENCIES {
            return Err(HappyChainError::TooManyDependencies(dependencies.len()));
        }
        Ok(Self {
            tx,
            priority,
            urgency,
            dependencies,
            created_at: Instant::now(),
            retry_count: 0,
        })
    }

    pub fn effective_priority(&self) -> u32 {
        let age_factor = self.created_at.elapsed().as_secs() as u32 / 60; // 1 point per minute
        self.priority + self.retry_count + age_factor
    }

    pub fn increment_retry(&mut self) {
        self.retry_count = self.retry_count.saturating_add(1);
    }

    pub fn can_retry(&self) -> bool {
        self.retry_count < MAX_RETRIES
    }
}

#[cfg(test)]
mod tests {
    use web3::types::TransactionParameters;
    use super::*;

    #[test]
    fn test_new_transaction() {
        let tx = TransactionParameters::default();
        let priority = 1;
        let urgency = TransactionUrgency::Medium;
        let dependencies = vec![H256::zero()];
        let transaction = Transaction::new(tx, priority, urgency, dependencies).unwrap();

        assert_eq!(transaction.priority, 1);
        assert_eq!(transaction.dependencies.len(), 1);
        assert_eq!(transaction.retry_count, 0);
    }

    #[test]
    fn test_too_many_dependencies() {
        let tx = TransactionParameters::default();
        let priority = 1;
        let urgency = TransactionUrgency::Medium;
        let dependencies = vec![H256::zero(); MAX_DEPENDENCIES + 1];
        assert!(matches!(
            Transaction::new(tx, priority, urgency, dependencies),
            Err(HappyChainError::TooManyDependencies(_))
        ));
    }

    #[test]
    fn test_effective_priority() {
        let tx = TransactionParameters::default();
        let priority = 1;
        let urgency = TransactionUrgency::Medium;
        let dependencies = vec![];
        let mut transaction = Transaction::new(tx, priority, urgency, dependencies).unwrap();

        assert_eq!(transaction.effective_priority(), 1);

        transaction.increment_retry();
        assert_eq!(transaction.effective_priority(), 2);

        // Note: Testing time-based priority increase is tricky and might lead to flaky tests
        // You might want to inject a clock or use a library like chrono::mock for more reliable testing
    }

    #[test]
    fn test_can_retry() {
        let tx = TransactionParameters::default();
        let priority = 1;
        let urgency = TransactionUrgency::Medium;
        let dependencies = vec![];
        let mut transaction = Transaction::new(tx, priority, urgency, dependencies).unwrap();

        assert!(transaction.can_retry());

        for _ in 0..MAX_RETRIES {
            transaction.increment_retry();
        }

        assert!(!transaction.can_retry());
    }
}
