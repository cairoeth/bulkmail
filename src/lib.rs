use alloy::transports::{RpcError, TransportErrorKind};
use thiserror::Error;

pub mod gas_price;
pub mod message;
pub mod priority_queue;
pub mod sender;
pub mod chain;
mod nonce_manager;

// Re-export main components for easier use
pub use gas_price::GasPriceManager;
pub use message::Message;
pub use priority_queue::PriorityQueue;
pub use sender::Sender;
pub use chain::Chain;
pub use nonce_manager::NonceManager;

pub const BLOCK_TIME: u32 = 2;
pub const POINTS_PER_BLOCK: u32 = 1;
pub const MAX_DEPENDENCIES: usize = 5;
pub const MAX_RETRIES: u32 = 3;

/// The main error type for the TM library
#[derive(Error, Debug)]
pub enum Error {
    // #[error("Web3 error: {0}")]
    // Web3Error(#[from] web3::Error),
    #[error("rpc error: {0}")]
    RpcError(#[from] RpcError<TransportErrorKind>),
    #[error("signing error")]
    SigningError(#[from] alloy::signers::Error),
    #[error("Too many dependencies: {0}")]
    TooManyDependencies(usize),
    #[error("Gas price error: {0}")]
    GasPriceError(String),
    #[error("Message expired")]
    MessageExpired,
    #[error("Retries exceeded")]
    RetriesExceeded,
    #[error("Fee increases exceeded")]
    FeeIncreasesExceeded,
    #[error("Gas price too low")]
    GasPriceTooLow,
    #[error("Simulation failed")]
    SimulationFailed,
    #[error("chain error: {0}")]
    ChainError(#[from] chain::Error),
}
