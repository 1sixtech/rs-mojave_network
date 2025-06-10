use std::{
	fmt,
	task::{Context, Poll},
};

use multiaddr::{Multiaddr, PeerId};
use thiserror::Error;

use crate::{
	AsyncReadWrite, ConnectionError, StreamProtocol,
	connection::{self, ConnectionId},
	stream_id::StreamId,
};

mod to_node;

pub use to_node::*;

#[derive(Debug, Error)]
pub enum PeerProtocolError {
	#[error("Connection denied: {0}")]
	ConnectionDenied(#[source] Box<dyn std::error::Error + Send + Sync>),
}

pub enum FromNode {}

pub type THandler<TProtocol> = <TProtocol as PeerProtocol>::Handler;
pub type THandlerFromEvent<TProtocol> = <THandler<TProtocol> as ProtocolHandler>::FromProtocol;
pub type THandlerToEvent<TProtocol> = <THandler<TProtocol> as ProtocolHandler>::ToProtocol;

pub enum ProtocolHandlerEvent<TEvent> {
	OutboundSubstreamRequest,
	NotifyProtocol(TEvent),
}

pub enum ConnectionEvent {
	NewInboundStream(Box<dyn AsyncReadWrite + Send + Unpin>),
	NewOutboundStream(Box<dyn AsyncReadWrite + Send + Unpin>),
	FailNegotiation(connection::negotiator::NegotiatorStreamError),
	AddressChange(Multiaddr),
}

pub trait ProtocolHandler: Send + 'static {
	type FromProtocol: fmt::Debug + Send + 'static;
	type ToProtocol: fmt::Debug + Send + 'static;
	type ProtocolInfoIter: IntoIterator<Item = StreamProtocol>;

	fn protocol_info(&self) -> Self::ProtocolInfoIter;

	fn on_connection_event(&mut self, event: ConnectionEvent);

	/// Call when we receive an event in the stream concerning this protocol of the protocol
	fn on_protocol_event(&mut self, event: Self::FromProtocol);

	/// Poll close
	fn poll_close(&mut self, _cx: &mut Context<'_>) -> Poll<Option<Self::ToProtocol>> {
		Poll::Ready(None)
	}

	fn poll(&mut self, cx: &mut Context<'_>) -> Poll<ProtocolHandlerEvent<Self::ToProtocol>>;
}

pub trait PeerProtocol: Send + 'static {
	type ToNode: Send + 'static;

	type Handler: ProtocolHandler;

	fn on_new_connection(
		&mut self,
		connection_id: ConnectionId,
		peer_id: PeerId,
		remote_addr: &Multiaddr,
		local_addr: Option<&Multiaddr>,
	) -> Result<Self::Handler, ConnectionError>;

	fn on_node_event(&mut self, event: FromNode);

	/// Poll for actions
	fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Action<Self::ToNode, THandlerFromEvent<Self>>>;
}

#[derive(Debug)]
pub enum Action<TEvent, THandlerEvent> {
	Event(TEvent),
	Connect(Multiaddr), // We initiate outbound connection
	OpenStream(PeerId),
	Send {
		peer: PeerId,
		stream: StreamId,
		data: Vec<u8>,
	},
	Notify(THandlerEvent),
	CloseStream {
		peer: PeerId,
		stream: StreamId,
	},
	Listen(Multiaddr),
	Nothing,
}
