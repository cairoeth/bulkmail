use std::time::Instant;
use web3::types::{Address, Bytes, H256, U256};
use crate::{Error, Result, MAX_DEPENDENCIES, MAX_RETRIES};

const BLOCK_TIME: u32 = 12; // In seconds
const POINTS_PER_BLOCK: u32 = 1;

#[derive(Debug, Clone)]
pub struct Message {
    pub from: Address,
    pub to: Option<Address>,

    pub gas: U256,
    pub value: Option<U256>,
    pub data: Option<Bytes>,
    pub priority: u32,
    pub dependencies: Vec<H256>,

    pub created_at: Instant,
    pub retry_count: u32,
}

impl Message {
    pub fn new(
        from: Address,
        to: Option<Address>,
        gas: U256,
        value: Option<U256>,
        data: Option<Bytes>,
        priority: u32,
        dependencies: Vec<H256>,
    ) -> Result<Self> {
        if dependencies.len() > MAX_DEPENDENCIES {
            return Err(Error::TooManyDependencies(dependencies.len()));
        }
        Ok(Self {
            from,
            to,
            gas,
            value,
            data,
            priority,
            dependencies,
            created_at: Instant::now(),
            retry_count: 0,
        })
    }

    pub fn effective_priority(&self) -> u32 {
        let age = self.created_at.elapsed().as_secs() as u32;
        let age_factor = age * POINTS_PER_BLOCK / BLOCK_TIME; // 1 point per block
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
    use crate::MAX_DEPENDENCIES;
    use super::*;

    #[test]
    fn test_new_message() {
        let from = Address::zero();
        let to = Some(Address::zero());
        let gas = U256::from(21_000);
        let value = Some(U256::zero());
        let data = None;
        let priority = 1;
        let dependencies = vec![H256::zero()];
        let message = Message::new(from, to, gas, value, data, priority, dependencies).unwrap();

        assert_eq!(message.priority, 1);
        assert_eq!(message.dependencies.len(), 1);
        assert_eq!(message.retry_count, 0);
    }

    #[test]
    fn test_too_many_dependencies() {
        let from = Address::zero();
        let to = Some(Address::zero());
        let gas = U256::from(21_000);
        let value = Some(U256::zero());
        let data = None;
        let priority = 1;
        let dependencies = vec![H256::zero(); MAX_DEPENDENCIES + 1];
        assert!(matches!(
            Message::new(from, to, gas, value, data, priority, dependencies),
            Err(Error::TooManyDependencies(_))
        ));
    }

    #[test]
    fn test_effective_priority() {
        let from = Address::zero();
        let to = Some(Address::zero());
        let gas = U256::from(21_000);
        let value = Some(U256::zero());
        let data = None;
        let priority = 1;
        let dependencies = vec![];
        let mut message = Message::new(from, to, gas, value, data, priority, dependencies).unwrap();

        assert_eq!(message.effective_priority(), 1);

        message.increment_retry();
        assert_eq!(message.effective_priority(), 2);

        // Simulate passage of time
        // TODO: Add a way to mock time
        // std::thread::sleep(Duration::from_secs(13));
        // assert_eq!(message.effective_priority(), 3);
    }

    #[test]
    fn test_can_retry() {
        let from = Address::zero();
        let to = Some(Address::zero());
        let gas = U256::from(21_000);
        let value = Some(U256::zero());
        let data = None;
        let priority = 1;
        let dependencies = vec![];
        let mut message = Message::new(from, to, gas, value, data, priority, dependencies).unwrap();

        assert!(message.can_retry());

        for _ in 0..MAX_RETRIES {
            message.increment_retry();
        }

        assert!(!message.can_retry());
    }
}
