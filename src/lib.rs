use thiserror::Error;
use web3::types::Address;

pub mod gas_price;
pub mod message;
pub mod priority_queue;
pub mod sender;

// Re-export main components for easier use
pub use gas_price::GasPriceManager;
pub use message::Message;
pub use priority_queue::PriorityQueue;
pub use sender::Sender;

pub const SENDER_ADDRESS: Address = Address::zero(); // Replace with actual address in production

pub const BLOCK_TIME: u32 = 2;
pub const POINTS_PER_BLOCK: u32 = 1;
pub const MAX_DEPENDENCIES: usize = 5;
pub const MAX_RETRIES: u32 = 3;

/// The main error type for the TM library
#[derive(Error, Debug)]
pub enum Error {
    #[error("Web3 error: {0}")]
    Web3Error(#[from] web3::Error),
    #[error("Too many dependencies: {0}")]
    TooManyDependencies(usize),
    #[error("Gas price error: {0}")]
    GasPriceError(String),
    #[error("Message expired")]
    MessageExpired,
    #[error("Retries exceeded")]
    RetriesExceeded,
    #[error("Gas price too low")]
    GasPriceTooLow,
}
