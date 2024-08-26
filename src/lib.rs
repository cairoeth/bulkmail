use alloy::transports::{RpcError, TransportErrorKind};
use thiserror::Error;

pub mod message;
pub mod sender;
pub mod chain;
mod gas_price;
mod priority_queue;
mod nonce_manager;

// Re-export main components for easier use
pub use chain::Chain;
pub use sender::Sender;
pub use message::Message;
pub(crate) use gas_price::GasPriceManager;
pub(crate) use nonce_manager::NonceManager;
pub(crate) use priority_queue::PriorityQueue;

/// The main error type for the TM library
#[derive(Error, Debug)]
pub enum Error {
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
