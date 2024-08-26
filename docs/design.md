# Bulkmail

A parallel transaction sender for Ethereum.

## 1. Introduction

Bulkmail is a robust Ethereum transaction management system designed to handle concurrent transaction sending to a rapid EVM blockchain. This document outlines the architecture, key components, and design decisions made during the development process.

## 2. Problem Statement

The core problem Bulkmail aims to solve is getting transactions included on the blockchain promptly and efficiently. There are a number of reasons a transaction might be slow to be confirmed, or dropped/rejected altogether which much be accounted for. Our goal is to get large amounts of transactions landed on a fast moving chain.

### 2.1 Transaction Lifecycle

1. **Generate**: Create a transaction with a nonce, gas price, gas limit, to, value, and data.
2. **Send**: Send the transaction to a node or RPC.
3. **Propagate**: The transaction is gossiped around the network.
4. **Mempool**: If it passes basic checks, the transaction is added to the mempool.
5. **Mine**: The transaction is included in a block. This requires a tranactions to be correct and pay a high enough tip
   to get included.

### 2.2 Potential Issues Landing On-chain

Roughly in order they would occur during the transaction lifecycle:

- **Malformed**: Correct code should never form a malformed transaction; making it the easiest to deal with.
    - **Strategy:** Use strongly typed code and tests to ensure well-formed transactions.
- **Gas price too low**: A common issue. The trickiest part of solving this is ensuring the gas isn't dramatically
  higher than necessary.
    - **Strategy:** Monitor the chain's base fee and congestion levels, and adjust the gas price accordingly.
- **Insufficient funds**: Largely application dependent, however all systems need to ensure they have enough funds to
  cover the transaction.
    - Managing funds is outside the scope of this system for now.
- **Nonce Too Low**: Either this transaction has already landed or we're incorrectly reusing an old nonce.
    - **Strategy:** Track the confirmed and in-flight nonces combined with deferring binding nonces to messages until
      right before sending
- **Nonce Too High**: We've missed a transaction or one of our transactions unconfirmed (possibly stuck or dropped).
    - **Strategy:** Track the confirmed and in-flight nonces and back-fill any gaps caused by dropped transactions.
- **Reverted**: Either a bug in the code or an unexpected state change in the chain.
    - **Strategy:** Retry failed transactions up to a maximum number of times and then log failures for further
      investigation.
- **Gas limit too low**: The transaction is too complex for the gas limit provided.
    - **Strategy:** Increase the gas limit and retry the transaction up to a maximum number of times. A future
      enhancement could be to estimate the gas limit by simulating the transaction against the RPC.
- **Dropped**: The transaction was dropped by the network.
    - **Strategy:** Retry failed transactions up to a maximum number of times.

## 3. High-level features

- Handles sending transactions to a rapid EVM blockchain.
- Can send multiple transactions concurrently.
- Implements a priority system for transactions.
- Includes a mechanism for replacing stuck transactions.
- Manages nonces to ensure proper transaction ordering.
- Late-binds nonces and gas prices to messages for flexible processing.

## 4. System Architecture

The system consists of several key components:

1. **Sender**: Manages the overall transaction sending process.
2. **Message**: Represents the intent to send a transaction.
3. **PriorityQueue**: Manages the ordering of messages to be processed.
4. **GasPriceManager**: Handles gas price calculations and adjustments.
5. **NonceManager**: Manages nonce assignment and tracking.
6. **Chain**: Wrapper around Alloy's RpcClient for blockchain interaction.

### 4.1 Sender

The `Sender` is the core component of the system. It:
- Maintains a queue of pending messages
- Processes messages in order of priority
- Handles message dependencies
- Manages nonce assignment
- Creates and sends transactions based on messages
- Interacts with the Ethereum network via Alloy
- Monitors and replaces stuck transactions

### 4.2 Message

The `Message` struct encapsulates:
- Essential transaction data (to, value, data, gas, etc.)
- Priority level
- Deadline for time-sensitive transactions
- Retry count and creation timestamp

### 4.3 PriorityQueue

Manages messages based on their effective priority, which considers:
- Base priority
- Time in queue
- Number of retries
- Proximity to deadline

### 4.4 GasPriceManager

Calculates and adjusts gas prices based on:
- Network congestion
- Message priority
- Recent confirmation times

### 4.5 NonceManager

Manages nonce assignment and tracking:
- Keeps track of the current nonce
- Manages in-flight nonces
- Synchronizes with the blockchain

### 4.6 Chain

Wraps Alloy's RpcClient to provide:
- Blockchain interactions (send transactions, get receipts, etc.)
- Subscription to new blocks

## 5. Key Considerations and Design Decisions

### 5.1 Message-based Architecture

- **Challenge**: Being able to adjust transaction parameters (like gas price) dynamically sending.
- **Solution**: Introduced a `Message` concept that represents the intent to send a transaction, separate from the actual `Transaction` object.

### 5.2 Gas Price Management

- **Challenge**: Adapting to network congestion and ensuring timely transaction processing.
- **Solution**: Dynamic gas price calculation based on network congestion and message priority.

### 5.3 Nonce Management

- **Challenge**: Ensure proper nonce ordering while allowing for concurrent message processing. Prevent an unmarketable or incorrect transaction block following transactions.
- **Solution**: Implemented a `NonceManager` that tracks current and in-flight nonces. Aggressively adjust fees to ensure fast confirmations.

### 5.4 Transaction Replacement

- **Challenge**: Handling stuck transactions in the mempool.
- **Solution**: Implemented a replacement strategy that increases gas price for stuck transactions.

### 5.5 Deadline System

- **Challenge**: Handling time-sensitive transactions that become invalid after a certain block number.
- **Solution**: Incorporated a deadline field in the `Message` struct and adjusted the priority calculation to account for approaching deadlines.

### 5.6 Concurrency Management

- **Challenge**: Managing multiple in-flight transactions without overwhelming the system.
- **Solution**: Implemented a semaphore-based approach to limit the number of concurrent transactions.

## 6. System Transaction Lifecycle

1. **Message Creation**: A `Message` is created with transaction intent and priority.
2. **Queuing**: The message is added to the `PriorityQueue`.
3. **Processing**: The `Sender` processes messages from the queue based on effective priority.
4. **Nonce Assignment**: The `NonceManager` assigns the next available nonce just before transaction creation.
5. **Gas Price Calculation**: The `GasPriceManager` determines appropriate gas prices based on network conditions and message priority.
6. **Transaction Creation and Sending**: The `Sender` creates an Ethereum transaction and sends it via the `Chain`.
7. **Monitoring**: The `Sender` monitors the transaction for confirmation or timeout.
8. **Confirmation/Replacement**: If confirmed, the transaction is marked as successful. If stuck, it may be replaced with a higher gas price.

## 7. Error Handling and Recovery

- **Retry Mechanism**: Messages that fail to be processed are retried up to a maximum number of times.
- **Gas Price Adjustment**: Stuck transactions are replaced with higher gas prices to increase the likelihood of inclusion.
- **Nonce Synchronization**: The `NonceManager` periodically syncs with the blockchain to ensure accurate nonce tracking.
- **Error Logging**: Comprehensive error logging is implemented throughout the system for debugging and monitoring.

## 8. Future Improvements

- Implement more sophisticated gas estimation techniques.
- Implement a more robust error recovery system for long-term reliability.
- Implement a more sophisticated congestion detection and avoidance system.

## 9. Conclusion

Bulkmail provides a flexible and efficient solution for managing concurrent Ethereum transactions. The message-based architecture, combined with dynamic priority management, gas price adjustment, and transaction replacement strategies, allows for robust handling of time-sensitive and high-volume transaction scenarios. The system is designed to be adaptable to changing network conditions and can be further enhanced to meet specific application requirements.
