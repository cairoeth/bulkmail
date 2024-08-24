use crate::{Error, BLOCK_TIME, MAX_DEPENDENCIES, MAX_RETRIES, POINTS_PER_BLOCK};
use chrono::{DateTime, Utc};
use std::time::Instant;
use web3::types::{Address, Bytes, H256, U256};

#[derive(Debug, Clone)]
pub struct Message {
    pub from: Address,
    pub to: Option<Address>,

    pub gas: Option<U256>,
    pub value: Option<U256>,
    pub data: Option<Bytes>,
    pub priority: u32,
    pub dependencies: Vec<H256>,
    pub deadline: Option<DateTime<Utc>>,

    pub created_at: Instant,
    pub retry_count: u32,
}

impl Message {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        from: Address,
        to: Option<Address>,
        gas: Option<U256>,
        value: Option<U256>,
        data: Option<Bytes>,
        priority: u32,
        dependencies: Vec<H256>,
        deadline: Option<DateTime<Utc>>,
    ) -> Result<Self, Error> {
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
            deadline,
            created_at: Instant::now(),
            retry_count: 0,
        })
    }

    pub fn effective_priority(&self) -> u32 {
        // Get the deadline factor.
        // If it's None then we don't influence the priority.
        // If it's Some(0) then the deadline has passed and we set the priority to 0.
        // Otherwise, we add the factor to the priority.
        let deadline_factor = match self.deadline_factor() {
            None => 0,
            Some(0) => return 0,
            Some(x) => x,
        };

        let age = self.created_at.elapsed().as_secs() as u32;
        let age_factor = age * POINTS_PER_BLOCK / BLOCK_TIME;

        self.priority + self.retry_count + age_factor + deadline_factor
    }

    /// Returns a factor to boost the priority based on the deadline.
    /// Returns None if there is no deadline.
    /// Returns Some(0) if the deadline has passed.
    fn deadline_factor(&self) -> Option<u32> {
        match self.deadline {
            None => None,
            Some(deadline) => {
                let now = Utc::now();
                if deadline <= now {
                    return Some(0);
                }

                let time_left = deadline - now;
                let seconds_left = time_left.num_seconds() as u32;
                if seconds_left < (BLOCK_TIME * 2) {
                    // Less than 2 blocks left, maximum priority boost
                    Some(50)
                } else if seconds_left < (BLOCK_TIME * 10) {
                    // Less than 10 blocks left, medium priority boost
                    Some(10)
                } else {
                    // More than 10 blocks left, low priority boost
                    Some(2)
                }
            }
        }
    }

    pub fn increment_retry(&mut self) {
        self.retry_count = self.retry_count.saturating_add(1);
    }

    pub fn can_retry(&self) -> bool {
        self.retry_count < MAX_RETRIES
    }

    pub fn is_expired(&self) -> bool {
        if let Some(deadline) = self.deadline {
            Utc::now() > deadline
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_DEPENDENCIES;

    #[test]
    fn test_new_message() {
        let from = Address::zero();
        let to = Some(Address::zero());
        let gas = Some(U256::from(21_000));
        let value = Some(U256::zero());
        let data = None;
        let priority = 1;
        let dependencies = vec![H256::zero()];
        let deadline = None;
        let message =
            Message::new(from, to, gas, value, data, priority, dependencies, deadline).unwrap();

        assert_eq!(message.priority, 1);
        assert_eq!(message.dependencies.len(), 1);
        assert_eq!(message.retry_count, 0);
    }

    #[test]
    fn test_too_many_dependencies() {
        let from = Address::zero();
        let to = Some(Address::zero());
        let gas = Some(U256::from(21_000));
        let value = Some(U256::zero());
        let data = None;
        let priority = 1;
        let dependencies = vec![H256::zero(); MAX_DEPENDENCIES + 1];
        let deadline = None;
        assert!(matches!(
            Message::new(from, to, gas, value, data, priority, dependencies, deadline),
            Err(Error::TooManyDependencies(_))
        ));
    }

    #[test]
    fn test_effective_priority() {
        let from = Address::zero();
        let to = Some(Address::zero());
        let gas = Some(U256::from(21_000));
        let value = Some(U256::zero());
        let data = None;
        let priority = 1;
        let dependencies = vec![];
        let deadline = None;
        let mut message =
            Message::new(from, to, gas, value, data, priority, dependencies, deadline).unwrap();

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
        let gas = Some(U256::from(21_000));
        let value = Some(U256::zero());
        let data = None;
        let priority = 1;
        let dependencies = vec![];
        let deadline = None;
        let mut message =
            Message::new(from, to, gas, value, data, priority, dependencies, deadline).unwrap();

        assert!(message.can_retry());

        for _ in 0..MAX_RETRIES {
            message.increment_retry();
        }

        assert!(!message.can_retry());
    }
}
