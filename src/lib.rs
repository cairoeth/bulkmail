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

/// The main error type for the TM library
#[derive(Debug)]
pub enum Error {
    Web3Error(web3::Error),
    TooManyDependencies(usize),
    GasPriceError(String),

}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Web3Error(e) => write!(f, "Web3 error: {}", e),
            Error::TooManyDependencies(count) => write!(f, "Too many dependencies: {}", count),
            Error::GasPriceError(msg) => write!(f, "Gas price error: {}", msg),
            // Handle other error types
        }
    }
}

impl std::error::Error for Error {}

impl From<web3::Error> for Error {
    fn from(err: web3::Error) -> Self {
        Error::Web3Error(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
