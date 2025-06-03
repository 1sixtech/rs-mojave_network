use multiaddr::Multiaddr;
use rs_mojave_network_core::transport::{Protocol, TransportError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("no protocols in multiaddr: {0}")]
	NoProtocolsInMultiaddr(Multiaddr),

	#[error("transport not found for protocol: {0:?}")]
	TransportNotFound(Protocol),

	#[error("transport error: {0}")]
	Transport(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),

	#[error("dial to: {0:?} error: {1}")]
	DialError(Multiaddr, TransportError<std::io::Error>),
}

#[derive(Debug, thiserror::Error)]
pub enum BuilderError {
	#[error("duplicate transport for protocol: {0:?}")]
	DuplicateTransport(Protocol),
}
