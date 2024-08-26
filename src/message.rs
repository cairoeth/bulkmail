use std::fmt::Display;
use crate::{BLOCK_TIME, MAX_RETRIES, POINTS_PER_BLOCK};
use chrono::{DateTime, Utc};
use std::time::Instant;
use alloy::primitives::{Address, Bytes, U256};

#[derive(Debug, Clone)]
pub struct Message {
    pub to: Option<Address>,
    pub value: U256,
    pub data: Bytes,
    pub gas: u128,

    pub priority: u32,
    pub deadline: Option<DateTime<Utc>>,

    pub created_at: Instant,
    pub retry_count: u32,
}

impl Message {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        to: Option<Address>,
        gas: u128,
        value: U256,
        data: Bytes,
        priority: u32,
        deadline: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            to,
            gas,
            value,
            data,
            priority,
            deadline,
            ..Default::default()
        }
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

    pub fn increment_retry(&mut self) -> bool {
        self.retry_count = self.retry_count.saturating_add(1);
        self.can_retry()
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

impl Default for Message{
    fn default() -> Self {
        Self{
            to: None,
            value: Default::default(),
            data: Default::default(),
            gas: 0,
            priority: 0,
            deadline: None,
            created_at: Instant::now(),
            retry_count: 0,
        }
    }

}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Message {{ to: {:?}, value: {}, gas: {}, priority: {}, deadline: {:?}, created_at: {:?}, retry_count: {} }}",
            self.to, self.value, self.gas, self.priority, self.deadline, self.created_at, self.retry_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_message() {
        let to = Some(Address::default());
        let gas = 21_000u128;
        let value = U256::from(0);
        let data = Bytes::default();
        let priority = 1;
        let deadline = None;
        let message =
            Message::new(to, gas, value, data, priority, deadline);

        assert_eq!(message.priority, 1);
        assert_eq!(message.retry_count, 0);
    }

    #[test]
    fn test_effective_priority() {
        let to = Some(Address::default());
        let gas = 21_000u128;
        let value = U256::from(0);
        let data = Bytes::default();
        let priority = 1;
        let dependencies = vec![];
        let deadline = None;
        let mut message =
            Message::new(to, gas, value, data, priority, deadline);

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
        let to = Some(Address::default());
        let gas = 21_000u128;
        let value = U256::from(0);
        let data = Bytes::default();
        let priority = 1;
        let dependencies = vec![];
        let deadline = None;
        let mut message =
            Message::new(to, gas, value, data, priority, deadline);

        assert!(message.can_retry());

        for _ in 0..MAX_RETRIES {
            message.increment_retry();
        }

        assert!(!message.can_retry());
    }
}
