# Bulkmail Architecture: Questions and Answers

## 1. How would you architect such a service?

Our architecture centers around a `Sender` component that manages a priority queue of `Message` objects, representing transaction intents. The system uses a `NonceManager` for nonce tracking, a `GasPriceManager` for dynamic gas price adjustments, and a `Chain` wrapper for blockchain interactions. This design allows for concurrent transaction processing while maintaining proper ordering and adapting to network conditions.

### Handling submission issues:

a) Contract execution reverting:
- Causes: Insufficient gas, invalid state transitions, or failed assertions.
- Solution: Implement a retry mechanism with increasing gas limits and log failures for further investigation.

b) Issues preventing prompt inclusion:
- Causes: Low gas price, network congestion, or nonce gaps.
- Solutions: Dynamic gas price adjustment, transaction replacement for stuck transactions, and proactive nonce management.

### Stack impact:
The library is written in Rust, leveraging its performance, safety features, and excellent Ethereum tooling ecosystem.

The Alloy create manages low-level RPC interactions and transaction signing. We handle nonce management, gas price strategies, while Alloy handles transaction monitoring and state loading. This stack gives us flexibility in transaction creation and monitoring, allowing for easy fee-bumping transaction replacement while still offloading the heavy work of watching for outcomes to Alloy.

## 2. How would deadlines impact the architecture?

To handle deadlines, we've implemented a deadline system within our `Message` struct. The `PriorityQueue` and `GasPriceManager` both consider deadline proximity when calculating transaction scheduling and priority fees, significantly boosting them for near-deadline transactions. This means they'll get put into transactions sooner and those transactions will have the max priority fee that we can afford.
