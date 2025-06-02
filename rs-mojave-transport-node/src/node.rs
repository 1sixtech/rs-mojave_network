use futures::stream::FusedStream;
use futures::{FutureExt, TryFutureExt};
use multiaddr::{Multiaddr, PeerId, Protocol as MultiaddrProtocol};
use rs_mojave_network_core::connection::ConnectionOrigin;
use rs_mojave_network_core::muxing::StreamMuxerBox;
use rs_mojave_network_core::transport::{self, TransportError};
use rs_mojave_network_core::{Protocol, Transport, transport::Boxed, transport::TransportEvent};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tracing::{error, info};

use crate::connection::ConnectionId;
use crate::error::Error;
use crate::peer::manager::{self, PeerEvent};
use crate::{NodeEvent, peer};

type TransportEventBoxed =
	TransportEvent<<transport::Boxed<(PeerId, StreamMuxerBox)> as Transport>::ListenerUpgrade, io::Error>;

pub struct Node<TProtocols> {
	pub peer_id: PeerId,
	transports: HashMap<Protocol, Boxed<(PeerId, StreamMuxerBox)>>,
	peer_manager: peer::manager::Manager,
	pending_events: VecDeque<NodeEvent>,

	protocols: TProtocols,
}

impl<TProtocols> Unpin for Node<TProtocols> {}

impl<TProtocols> Node<TProtocols> {
	pub fn new(
		peer_id: PeerId,
		protocols: TProtocols,
		transports: HashMap<Protocol, Boxed<(PeerId, StreamMuxerBox)>>,
	) -> Self {
		Self {
			peer_id,
			transports,
			protocols,
			pending_events: VecDeque::new(),
			peer_manager: manager::Manager::new(),
		}
	}

	pub async fn dial(&mut self, remote_peer_id: PeerId, remote_address: Multiaddr) -> Result<(), Error> {
		let connection_id = ConnectionId::next();
		info!(peer_id = %self.peer_id, %remote_peer_id, %remote_address, %connection_id, "Attempting to dial");

		let protocol = extract_protocol_from_multiaddr(&remote_address)?;

		let transport = self.transports.get_mut(&protocol).ok_or_else(|| {
			error!(peer_id = %self.peer_id, %remote_peer_id, %remote_address, ?protocol, "Transport not found for protocol");
			Error::TransportNotFound(protocol)
		})?;

		let dial = match transport.dial(remote_address.clone()) {
			Ok(fut) => fut.map_err(TransportError::Other).boxed(),
			Err(e) => futures::future::ready(Result::<(PeerId, StreamMuxerBox), _>::Err(e)).boxed(),
		};

		self.peer_manager.add_outgoing(dial, connection_id, remote_address);

		Ok(())
	}

	pub async fn listen(&mut self, address: Multiaddr) -> Result<(), Error> {
		let protocol = extract_protocol_from_multiaddr(&address)?;

		let transport = self.transports.get_mut(&protocol).ok_or_else(|| {
			error!(peer_id = %self.peer_id, %address, ?protocol, "Transport not found for protocol");
			Error::TransportNotFound(protocol)
		})?;

		transport
			.listen_on(address.clone())
			.inspect_err(|e| {
				error!(peer_id = %self.peer_id, %address, ?e, "Failed to listen");
			})
			.map_err(|e| Error::Transport(Box::new(e)))?;

		Ok(())
	}

	fn poll_next_event(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<NodeEvent> {
		let this = &mut *self;

		'outer: loop {
			if let Some(event) = this.pending_events.pop_front() {
				return Poll::Ready(event);
			}

			match this.peer_manager.poll(cx) {
				Poll::Pending => {}
				Poll::Ready(event) => {
					this.handle_peer_event(event);
					continue 'outer;
				}
			}

			for v in this.transports.values_mut() {
				match Pin::new(v).poll(cx) {
					Poll::Ready(event) => {
						this.handle_transport_event(event);
						continue 'outer;
					}
					Poll::Pending => {}
				}
			}

			return Poll::Pending;
		}
	}

	#[inline]
	fn handle_peer_event(&mut self, _event: PeerEvent) {
		match _event {
			PeerEvent::ConnectionEstablished {
				connection_origin,
				connection_id,
				peer_id,
				stream_muxer_box,
				established_in,
			} => self.handle_peer_event_connection_established(
				connection_id,
				connection_origin,
				peer_id,
				stream_muxer_box,
				established_in,
			),
			PeerEvent::PendingOutboundConnectionError { .. } => {}
			PeerEvent::PendingInboundConnectionError { .. } => {}
		}
	}

	#[inline]
	#[tracing::instrument(skip(self), level = "debug", name = "Node::handle_peer_event_connection_established")]
	fn handle_peer_event_connection_established(
		&mut self,
		connection_id: ConnectionId,
		connection_origin: ConnectionOrigin,
		peer_id: PeerId,
		stream_muxer_box: StreamMuxerBox,
		established_in: web_time::Duration,
	) {
		let node_event = NodeEvent::ConnectionEstablished { connection_id, peer_id };
		self.pending_events.push_back(node_event);
	}

	#[inline]
	fn handle_transport_event(&mut self, event: TransportEventBoxed) {
		match event {
			TransportEvent::Incoming {
				remote_addr,
				local_addr,
				upgrade,
			} => self.handle_transport_event_incoming(upgrade, remote_addr, local_addr),
			TransportEvent::ListenAddress { address } => self.handle_transport_event_listen_address(address),
			TransportEvent::AddressExpired { address } => self.handle_transport_event_address_expired(address),
			TransportEvent::ListenerClosed { reason } => self.handle_transport_event_listener_closed(reason),
			TransportEvent::ListenerError { error } => self.handle_transport_event_listener_error(error),
		}
	}

	#[inline]
	fn handle_transport_event_incoming<TFut>(&mut self, upgrade: TFut, remote_addr: Multiaddr, local_addr: Multiaddr)
	where
		TFut: Future<Output = Result<(PeerId, StreamMuxerBox), std::io::Error>> + Send + 'static,
	{
		let connection_id = ConnectionId::next();
		tracing::debug!(peer_id = %self.peer_id, %remote_addr, %connection_id, "Incoming connection");
		self.peer_manager
			.add_incoming(upgrade, connection_id, local_addr, remote_addr.clone());

		let node_event = NodeEvent::IncomingConnection {
			remote_address: remote_addr,
		};
		self.pending_events.push_back(node_event);
	}

	#[inline]
	fn handle_transport_event_listen_address(&mut self, address: Multiaddr) {
		tracing::debug!(peer_id = %self.peer_id, %address, "Listening on");
		let node_event = NodeEvent::NewListenAddr { address };
		self.pending_events.push_back(node_event);
	}

	#[inline]
	fn handle_transport_event_address_expired(&mut self, address: Multiaddr) {
		tracing::debug!(peer_id = %self.peer_id, %address, "Listen address expired");
		let node_event = NodeEvent::AddressExpired { address };
		self.pending_events.push_back(node_event);
	}

	#[inline]
	fn handle_transport_event_listener_closed(&mut self, reason: Result<(), io::Error>) {
		tracing::debug!(peer_id = %self.peer_id, ?reason, "Listener closed");
		let node_event = NodeEvent::ListenerClosed { reason };
		self.pending_events.push_back(node_event);
	}

	#[inline]
	fn handle_transport_event_listener_error(&mut self, error: io::Error) {
		tracing::debug!(peer_id = %self.peer_id, ?error, "Listener error");
		let node_event = NodeEvent::ListenerError { error };
		self.pending_events.push_back(node_event);
	}
}

impl<TProtocols> futures::Stream for Node<TProtocols> {
	type Item = NodeEvent;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.poll_next_event(cx).map(Some)
	}
}

impl<TProtocols> FusedStream for Node<TProtocols> {
	fn is_terminated(&self) -> bool {
		false
	}
}

fn extract_protocol_from_multiaddr(address: &Multiaddr) -> Result<Protocol, Error> {
	let components = address.iter();
	let mut p2p_protocol: Option<Protocol> = None;

	for component in components {
		if component == MultiaddrProtocol::WebTransport {
			p2p_protocol = Some(Protocol::WebTransport);
			break;
		}
	}
	p2p_protocol.ok_or_else(|| Error::NoProtocolsInMultiaddr(address.clone()))
}
