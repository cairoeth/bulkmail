use std::error::Error;
use web3::types::Address;

pub mod transaction;
pub mod priority_queue;
pub mod sender;
pub mod gas_price;

// Re-export main components for easier use
pub use transaction::Transaction;
pub use priority_queue::PriorityQueue;
pub use sender::Sender;
pub use gas_price::GasPriceManager;

pub const SENDER_ADDRESS: Address = Address::zero(); // Replace with actual address in production

/// The main error type for the happychain library
#[derive(Debug)]
pub enum HappyChainError {
    Web3Error(web3::Error),
    TooManyDependencies(usize),
    GasPriceError(String),
    // Add other error types as needed

}

impl std::fmt::Display for HappyChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HappyChainError::Web3Error(e) => write!(f, "Web3 error: {}", e),
            HappyChainError::TooManyDependencies(count) => write!(f, "Too many dependencies: {}", count),
            HappyChainError::GasPriceError(msg) => write!(f, "Gas price error: {}", msg),
            // Handle other error types
        }
    }
}

impl Error for HappyChainError {}

impl From<web3::Error> for HappyChainError {
    fn from(err: web3::Error) -> Self {
        HappyChainError::Web3Error(err)
    }
}

// Add other From implementations as needed

pub type Result<T> = std::result::Result<T, HappyChainError>;
