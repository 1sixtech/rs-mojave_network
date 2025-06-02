use std::io;

use rs_mojave_network_core::transport::TransportError;

pub(crate) mod manager;
pub(crate) mod task;

#[derive(Debug)]
pub(crate) enum PendingOutboundConnectionError {
	Aborted,
	Transport(TransportError<io::Error>),
}

#[derive(Debug)]
pub(crate) enum PendingInboundConnectionError {
	Aborted,
	Transport(TransportError<io::Error>),
}
