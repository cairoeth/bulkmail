use crate::transaction::Transaction;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug)]
pub struct PriorityQueue {
    heap: BinaryHeap<PrioritizedTransaction>,
}

#[derive(Debug)]
struct PrioritizedTransaction {
    transaction: Transaction,
    effective_priority: u32,
}

impl Eq for PrioritizedTransaction {}

impl PartialEq for PrioritizedTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.effective_priority == other.effective_priority
    }
}

impl Ord for PrioritizedTransaction {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for max-heap behavior
        other.effective_priority.cmp(&self.effective_priority)
    }
}

impl PartialOrd for PrioritizedTransaction {
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

    pub fn push(&mut self, transaction: Transaction) {
        let effective_priority = transaction.effective_priority();
        self.heap.push(PrioritizedTransaction { transaction, effective_priority });
    }

    pub fn pop(&mut self) -> Option<Transaction> {
        self.heap.pop().map(|pt| pt.transaction)
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

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
    use super::*;
    use crate::gas_price::TransactionUrgency;
    use web3::types::TransactionParameters;

    #[test]
    fn test_priority_queue() {
        let mut queue = PriorityQueue::new();

        let tx1 = Transaction::new(TransactionParameters::default(), 1, TransactionUrgency::Low, vec![]).unwrap();
        let tx2 = Transaction::new(TransactionParameters::default(), 2, TransactionUrgency::Medium, vec![]).unwrap();
        let tx3 = Transaction::new(TransactionParameters::default(), 3, TransactionUrgency::High, vec![]).unwrap();

        queue.push(tx1.clone());
        queue.push(tx2.clone());
        queue.push(tx3.clone());

        assert_eq!(queue.len(), 3);
        assert!(!queue.is_empty());

        // Transactions should come out in order of priority
        assert_eq!(queue.pop().unwrap().priority, 3);
        assert_eq!(queue.pop().unwrap().priority, 2);
        assert_eq!(queue.pop().unwrap().priority, 1);

        assert!(queue.is_empty());
    }

    #[test]
    fn test_prioritized_transaction_ordering() {
        let tx1 = Transaction::new(TransactionParameters::default(), 1, TransactionUrgency::Low, vec![]).unwrap();
        let tx2 = Transaction::new(TransactionParameters::default(), 2, TransactionUrgency::Medium, vec![]).unwrap();

        let pt1 = PrioritizedTransaction {
            transaction: tx1,
            effective_priority: 1,
        };

        let pt2 = PrioritizedTransaction {
            transaction: tx2,
            effective_priority: 2,
        };

        assert!(pt1 > pt2); // Remember, we want a max-heap, so higher priority should be "greater"
        assert_eq!(pt1.cmp(&pt2), Ordering::Greater);
    }
}
