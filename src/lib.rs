use std::error::Error;
use web3::types::Address;

pub mod priority_queue;
pub mod sender;
pub mod gas_price;
pub mod message;

// Re-export main components for easier use
pub use priority_queue::PriorityQueue;
pub use sender::Sender;
pub use gas_price::GasPriceManager;
pub use message::Message;

pub const SENDER_ADDRESS: Address = Address::zero(); // Replace with actual address in production

pub const MAX_DEPENDENCIES: usize = 5;
pub const MAX_RETRIES: u32 = 3;

/// The main error type for the happychain library
#[derive(Debug)]
pub enum TMError {
    Web3Error(web3::Error),
    TooManyDependencies(usize),
    GasPriceError(String),
    // Add other error types as needed

}

impl std::fmt::Display for TMError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TMError::Web3Error(e) => write!(f, "Web3 error: {}", e),
            TMError::TooManyDependencies(count) => write!(f, "Too many dependencies: {}", count),
            TMError::GasPriceError(msg) => write!(f, "Gas price error: {}", msg),
            // Handle other error types
        }
    }
}

impl Error for TMError {}

impl From<web3::Error> for TMError {
    fn from(err: web3::Error) -> Self {
        TMError::Web3Error(err)
    }
}

pub type Result<T> = std::result::Result<T, TMError>;
