use std::{
	convert::Infallible,
	pin::{Pin, pin},
};

use futures::{
	SinkExt, StreamExt,
	channel::{mpsc, oneshot},
	future::{Either, Future, poll_fn},
};
use multiaddr::{Multiaddr, PeerId};
use rs_mojave_network_core::{muxing::StreamMuxerBox, transport::TransportError};

use crate::{
	ConnectionError, PeerProtocolBis,
	connection::{self, ConnectionId},
	peer::{PendingInboundConnectionError, PendingOutboundConnectionError},
};

pub(crate) enum PendingPeerEvent {
	ConnectionEstablished {
		connection_id: ConnectionId,
		output: (PeerId, StreamMuxerBox),
	},
	PendingFailed {
		connection_id: ConnectionId,
		error: Either<PendingOutboundConnectionError, PendingInboundConnectionError>,
	},
}

pub(crate) enum PeerEvent {}

pub(crate) async fn new_pending_outgoing_peer<TFut>(
	future: TFut,
	connection_id: ConnectionId,
	abort_receiver: oneshot::Receiver<Infallible>,
	mut events: mpsc::Sender<PendingPeerEvent>,
) where
	TFut: Future<Output = Result<(PeerId, StreamMuxerBox), TransportError<std::io::Error>>> + Send + 'static,
{
	tracing::debug!("New outgoing pending peer");
	match futures::future::select(abort_receiver, Box::pin(future)).await {
		Either::Left((Err(oneshot::Canceled), _)) => {
			let _ = events
				.send(PendingPeerEvent::PendingFailed {
					connection_id,
					error: Either::Left(PendingOutboundConnectionError::Aborted),
				})
				.await;
		}
		Either::Left((Ok(v), _)) => rs_mojave_network_core::util::unreachable(v),
		Either::Right((Ok(output), _)) => {
			let _ = events
				.send(PendingPeerEvent::ConnectionEstablished { connection_id, output })
				.await;
		}
		Either::Right((Err(e), _)) => {
			let _ = events
				.send(PendingPeerEvent::PendingFailed {
					connection_id,
					error: Either::Left(PendingOutboundConnectionError::Transport(e)),
				})
				.await;
		}
	}
}

pub(crate) async fn new_pending_inbound_peer<TFut>(
	future: TFut,
	connection_id: ConnectionId,
	abort_receiver: oneshot::Receiver<Infallible>,
	mut events: mpsc::Sender<PendingPeerEvent>,
) where
	TFut: Future<Output = Result<(PeerId, StreamMuxerBox), std::io::Error>> + Send + 'static,
{
	tracing::debug!("New inbound pending peer");
	match futures::future::select(abort_receiver, Box::pin(future)).await {
		Either::Left((Err(oneshot::Canceled), _)) => {
			let _ = events
				.send(PendingPeerEvent::PendingFailed {
					connection_id,
					error: Either::Right(PendingInboundConnectionError::Aborted),
				})
				.await;
		}
		Either::Left((Ok(v), _)) => rs_mojave_network_core::util::unreachable(v),
		Either::Right((Ok(output), _)) => {
			let _ = events
				.send(PendingPeerEvent::ConnectionEstablished { connection_id, output })
				.await;
		}
		Either::Right((Err(e), _)) => {
			let _ = events
				.send(PendingPeerEvent::PendingFailed {
					connection_id,
					error: Either::Right(PendingInboundConnectionError::Transport(TransportError::Other(e))),
				})
				.await;
		}
	}
}

pub(crate) enum Command<T> {
	/// Notify the protocol handler of some event.
	NotifyProtocol(T),
	/// Close the connection.
	Close,
}

pub enum EstablishedConnectionEvent<ToProtocol> {
	Closed {
		connection_id: ConnectionId,
		peer_id: PeerId,
		error: Option<ConnectionError>,
	},
	AddressChange {
		connection_id: ConnectionId,
		peer_id: PeerId,
		new_address: Multiaddr,
	},
	Notify {
		connection_id: ConnectionId,
		peer_id: PeerId,
		event: ToProtocol,
	},
}

/// Will poll the command receiver and the given connection and will forward them to the event_sender.
pub async fn new_established_connection<TProtocol>(
	connection_id: ConnectionId,
	peer_id: PeerId,
	mut connection: crate::connection::Connection<TProtocol>,
	mut command_receiver: mpsc::Receiver<Command<TProtocol::FromProtocol>>,
	mut event_sender: mpsc::Sender<EstablishedConnectionEvent<TProtocol::ToProtocol>>,
) where
	TProtocol: PeerProtocolBis,
{
	loop {
		match futures::future::select(
			command_receiver.next(),
			poll_fn(|cx| Pin::new(&mut connection).poll(cx)),
		)
		.await
		{
			Either::Left((None, _)) => return,
			Either::Left((Some(cmd), _)) => match cmd {
				Command::NotifyProtocol(protocol_event) => connection.on_protocol_event(protocol_event),
				Command::Close => {
					handle_close(connection_id, peer_id, connection, command_receiver, event_sender, None);
					return;
				}
			},
			Either::Right((event, _)) => match event {
				Ok(connection::Event::Handler(event)) => {
					let _ = event_sender
						.send(EstablishedConnectionEvent::Notify {
							connection_id,
							peer_id,
							event,
						})
						.await;
				}
				Ok(connection::Event::AddressChange(new_address)) => {
					let _ = event_sender
						.send(EstablishedConnectionEvent::AddressChange {
							connection_id,
							peer_id,
							new_address,
						})
						.await;
				}
				Err(error) => {
					handle_close(
						connection_id,
						peer_id,
						connection,
						command_receiver,
						event_sender,
						Some(error),
					);
					return;
				}
			},
		}
	}
}

pub async fn handle_close<TProtocol>(
	connection_id: ConnectionId,
	peer_id: PeerId,
	connection: crate::connection::Connection<TProtocol>,
	mut command_receiver: mpsc::Receiver<Command<TProtocol::FromProtocol>>,
	mut event_sender: mpsc::Sender<EstablishedConnectionEvent<TProtocol::ToProtocol>>,
	error: Option<ConnectionError>,
) where
	TProtocol: PeerProtocolBis,
{
	command_receiver.close();
	let (remaining_events, closing_muxer) = connection.close();

	event_sender
		.send_all(&mut remaining_events.map(|event| {
			Ok(EstablishedConnectionEvent::Notify {
				connection_id,
				peer_id,
				event,
			})
		}))
		.await;

	let error = if error.is_none() {
		closing_muxer.await.err().map(ConnectionError::IO)
	} else {
		error
	};

	event_sender
		.send(EstablishedConnectionEvent::Closed {
			connection_id,
			peer_id,
			error,
		})
		.await;
}
