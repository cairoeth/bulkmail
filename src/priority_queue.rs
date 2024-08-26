use crate::message::Message;
use std::{cmp::Ordering, collections::BinaryHeap};

#[derive(Debug)]
pub(crate) struct PriorityQueue {
    heap: BinaryHeap<PrioritizedMessage>,
}

#[derive(Debug)]
struct PrioritizedMessage {
    message: Message,
    effective_priority: u32,
}

impl Eq for PrioritizedMessage {}

impl PartialEq for PrioritizedMessage {
    fn eq(&self, other: &Self) -> bool {
        self.effective_priority == other.effective_priority
    }
}

impl Ord for PrioritizedMessage {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for max-heap behavior
        // other.effective_priority.cmp(&self.effective_priority)
        self.effective_priority.cmp(&other.effective_priority)
    }
}

impl PartialOrd for PrioritizedMessage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PriorityQueue {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
        }
    }

    pub fn push(&mut self, message: Message) {
        let effective_priority = message.effective_priority();
        self.heap.push(PrioritizedMessage {
            message,
            effective_priority,
        });
    }

    pub fn pop(&mut self) -> Option<Message> {
        self.heap.pop().map(|pm| pm.message)
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.heap.len()
    }
}

impl Default for PriorityQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::Address;
    use super::*;
    use crate::{Error, Message};

    #[test]
    fn test_priority_queue() -> Result<(), Error> {
        let mut queue = PriorityQueue::new();

        let msg1 = Message {
            to: Some(Address::default()),
            ..Default::default()
        };

        let msg2 = Message {
            to: Some(Address::default()),
            priority: 1,
            ..Default::default()
        };

        let msg3 = Message {
            to: Some(Address::default()),
            priority: 2,
            ..Default::default()
        };

        queue.push(msg1);
        queue.push(msg2);
        queue.push(msg3);

        assert_eq!(queue.len(), 3);
        assert!(!queue.is_empty());

        // Messages should come out in order of priority
        assert_eq!(queue.pop().unwrap().priority, 2);
        assert_eq!(queue.pop().unwrap().priority, 1);
        assert_eq!(queue.pop().unwrap().priority, 0);
        assert!(queue.is_empty());

        Ok(())
    }

    #[test]
    fn test_prioritized_message_ordering() -> Result<(), Error> {
        let msg1 = Message{
            priority: 1,
            ..Default::default()
        };
        let msg2 = Message{
            priority: 2,
            ..Default::default()
        };

        let pm1 = PrioritizedMessage {
            message: msg1,
            effective_priority: 1,
        };

        let pm2 = PrioritizedMessage {
            message: msg2,
            effective_priority: 2,
        };

        assert!(pm1 < pm2);
        assert_eq!(pm1.cmp(&pm2), Ordering::Less);
        Ok(())
    }
}
