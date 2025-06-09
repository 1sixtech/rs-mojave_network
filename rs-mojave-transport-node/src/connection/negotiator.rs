use serde::{Deserialize, Serialize};

use crate::StreamProtocol;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamProtocols(pub Vec<StreamProtocol>);

mod inbound;
mod outbound;

pub use inbound::*;
pub use outbound::*;

#[derive(Debug, thiserror::Error)]
pub enum NegotiatorStreamError {
	#[error("Timeout while negotiating timeout")]
	Timeout,

	#[error("I/O error while negotiating")]
	IoError(#[from] std::io::Error),

	#[error("Negotiation failed")]
	NegotiationFailed,
}
