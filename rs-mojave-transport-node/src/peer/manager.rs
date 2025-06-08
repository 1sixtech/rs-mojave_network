use futures::{
	StreamExt,
	channel::{mpsc, oneshot},
	future::{BoxFuture, Either},
	prelude::*,
	stream::{FuturesUnordered, SelectAll},
};
use multiaddr::{Multiaddr, PeerId};
use rs_mojave_network_core::{
	connection::ConnectionOrigin,
	muxing::{StreamMuxerBox, StreamMuxerExt},
	transport::TransportError,
};
use std::{
	collections::HashMap,
	convert::Infallible,
	task::{Context, Poll, Waker},
};
use tracing::Instrument;
use web_time::Instant;

use crate::{
	ProtocolHandler,
	connection::{Connection, ConnectionId},
	executor::{Executor, get_executor},
	peer::{PendingInboundConnectionError, PendingOutboundConnectionError, task},
};

struct TaskExecutor(Box<dyn Executor + Send>);

impl TaskExecutor {
	pub fn new() -> Self {
		Self(Box::new(get_executor()))
	}

	#[track_caller]
	pub fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
		let future = future.boxed();

		self.0.exec(future);
	}
}

pub struct EstablishedConnection<TFromProtocol> {
	connection_origin: ConnectionOrigin,

	sender: mpsc::Sender<task::Command<TFromProtocol>>,
}

pub struct Manager<THandler>
where
	THandler: ProtocolHandler,
{
	pending_peer_events_tx: mpsc::Sender<task::PendingPeerEvent>,
	pending_peer_events_rx: mpsc::Receiver<task::PendingPeerEvent>,
	new_peer_dropped_listeners: FuturesUnordered<oneshot::Receiver<StreamMuxerBox>>,
	peer_events: SelectAll<mpsc::Receiver<task::PeerEvent>>,
	task_executor: TaskExecutor,
	pending: HashMap<ConnectionId, PendingPeer>,
	established: HashMap<PeerId, HashMap<ConnectionId, EstablishedConnection<THandler::FromProtocol>>>,

	// connections
	no_established_connections_waker: Option<Waker>,
}

impl<THandler> Manager<THandler>
where
	THandler: ProtocolHandler,
{
	pub fn new() -> Self {
		let (pending_peer_events_tx, pending_peer_events_rx) = mpsc::channel(0);

		Self {
			pending_peer_events_tx,
			pending_peer_events_rx,
			new_peer_dropped_listeners: Default::default(),
			peer_events: Default::default(),
			task_executor: TaskExecutor::new(),
			pending: Default::default(),
			established: Default::default(),
			no_established_connections_waker: None,
		}
	}

	pub(crate) fn add_incoming<TFut>(
		&mut self,
		upgrage: TFut,
		connection_id: ConnectionId,
		local_addr: Multiaddr,
		remote_addr: Multiaddr,
	) where
		TFut: Future<Output = Result<(PeerId, StreamMuxerBox), std::io::Error>> + Send + 'static,
	{
		let (abort_notifier, abort_receiver) = oneshot::channel();

		let span = tracing::debug_span!(parent: tracing::Span::none(), "new_incoming_connection", remote_addr = %remote_addr, id = %local_addr);
		span.follows_from(tracing::Span::current());

		self.task_executor.spawn(
			task::new_pending_inbound_peer(
				upgrage,
				connection_id,
				abort_receiver,
				self.pending_peer_events_tx.clone(),
			)
			.instrument(span),
		);

		self.pending.insert(
			connection_id,
			PendingPeer {
				connection_origin: ConnectionOrigin::Listener {
					local_addr,
					remote_addr,
				},
				abort_notifier: Some(abort_notifier),
				accepted_at: Instant::now(),
			},
		);
	}

	pub(crate) fn add_outgoing(
		&mut self,
		dial: BoxFuture<'static, Result<(PeerId, StreamMuxerBox), TransportError<std::io::Error>>>,
		connection_id: ConnectionId,
		remote_addr: Multiaddr,
	) {
		let (abort_notifier, abort_receiver) = oneshot::channel();

		let span =
			tracing::debug_span!(parent: tracing::Span::none(), "new_outgoing_connection", remote_addr = %remote_addr);
		span.follows_from(tracing::Span::current());

		self.task_executor.spawn(
			task::new_pending_outgoing_peer(dial, connection_id, abort_receiver, self.pending_peer_events_tx.clone())
				.instrument(span),
		);

		self.pending.insert(
			connection_id,
			PendingPeer {
				connection_origin: ConnectionOrigin::Dialer { remote_addr },
				abort_notifier: Some(abort_notifier),
				accepted_at: Instant::now(),
			},
		);
	}

	pub(crate) fn spawn_connection(
		&mut self,
		connection_id: ConnectionId,
		peer_id: PeerId,
		connection_origin: ConnectionOrigin,
		connection: StreamMuxerBox,
		protocol: THandler,
	) {
		let connections = self.established.entry(peer_id).or_default();

		// TODO: replace with config vars
		let (command_sender, command_receiver) = mpsc::channel(16);
		// TODO: replace with config vars
		let (event_sender, event_receiver) = mpsc::channel(16);

		let established_connection = EstablishedConnection {
			connection_origin,
			sender: command_sender,
		};
		connections.insert(connection_id, established_connection);
		let connection = Connection::new(protocol, connection);

		let span = tracing::debug_span!(parent: tracing::Span::none(), "new_established_connection", %connection_id, peer = %peer_id);
		span.follows_from(tracing::Span::current());

		self.task_executor.spawn(task::new_established_connection(
			connection_id,
			peer_id,
			connection,
			command_receiver,
			event_sender,
		));
	}

	pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<PeerEvent> {
		match self.peer_events.poll_next_unpin(cx) {
			Poll::Pending => {}
			Poll::Ready(None) => {
				self.no_established_connections_waker = Some(cx.waker().clone());
			}
			Poll::Ready(Some(event)) => {
				return self.handle_peer_event(event);
			}
		}

		loop {
			if let Poll::Ready(Some(event)) = self.new_peer_dropped_listeners.poll_next_unpin(cx) {
				if let Ok(connection) = event {
					self.task_executor.spawn(async move {
						if let Err(e) = connection.close().await {
							tracing::error!(?e, "Failed to close connection");
						}
					});
				}
				continue;
			}

			let event = match self.pending_peer_events_rx.poll_next_unpin(cx) {
				Poll::Ready(Some(event)) => event,
				Poll::Ready(None) => unreachable!("Shouldn't be reachable"),
				Poll::Pending => break,
			};

			return self.handle_pending_peer_event(event);
		}

		//self.executor.advance_local(cx);

		Poll::Pending
	}

	#[inline]
	fn handle_peer_event(&mut self, _event: task::PeerEvent) -> Poll<PeerEvent> {
		todo!()
	}

	#[inline]
	fn handle_pending_peer_event(&mut self, event: task::PendingPeerEvent) -> Poll<PeerEvent> {
		match event {
			task::PendingPeerEvent::ConnectionEstablished { connection_id, output } => {
				self.handle_pending_peer_event_connection_established(connection_id, output)
			}
			task::PendingPeerEvent::PendingFailed { connection_id, error } => {
				self.handle_pending_peer_event_pending_failed(connection_id, error)
			}
		}
	}

	#[inline]
	fn handle_pending_peer_event_connection_established(
		&mut self,
		connection_id: ConnectionId,
		output: (PeerId, StreamMuxerBox),
	) -> Poll<PeerEvent> {
		let PendingPeer {
			connection_origin,
			abort_notifier: _,
			accepted_at,
		} = self
			.pending
			.remove(&connection_id)
			.expect("Entry in `self.pending` not found for pending peer");

		let (peer_id, stream_muxer_box) = output;

		let established_in = accepted_at.elapsed();

		Poll::Ready(PeerEvent::ConnectionEstablished {
			connection_origin,
			connection_id,
			peer_id,
			stream_muxer_box,
			established_in,
		})
	}

	#[inline]
	fn handle_pending_peer_event_pending_failed(
		&mut self,
		connection_id: ConnectionId,
		error: Either<PendingOutboundConnectionError, PendingInboundConnectionError>,
	) -> Poll<PeerEvent> {
		self.pending.remove(&connection_id);
		// connection is lost at that point, so we remove it from the ConnectionId registry
		ConnectionId::remove(connection_id);
		match error {
			Either::Left(error) => Poll::Ready(PeerEvent::PendingOutboundConnectionError { connection_id, error }),
			Either::Right(error) => Poll::Ready(PeerEvent::PendingInboundConnectionError { connection_id, error }),
		}
	}
}

#[derive(Debug)]
pub(crate) enum PeerEvent {
	PendingOutboundConnectionError {
		connection_id: ConnectionId,
		error: PendingOutboundConnectionError,
	},
	PendingInboundConnectionError {
		connection_id: ConnectionId,
		error: PendingInboundConnectionError,
	},

	ConnectionEstablished {
		connection_origin: ConnectionOrigin,
		connection_id: ConnectionId,
		peer_id: PeerId,
		stream_muxer_box: StreamMuxerBox,
		established_in: web_time::Duration,
	},
}

struct PendingPeer {
	connection_origin: ConnectionOrigin,

	/// When dropped, notifies the task which then knows to terminate.
	abort_notifier: Option<oneshot::Sender<Infallible>>,
	/// The moment we became aware of this possible connection, useful for timing metrics.
	accepted_at: Instant,
}

impl PendingPeer {
	/// Aborts the connection attempt, closing the connection.
	fn abort(&mut self) {
		if let Some(notifier) = self.abort_notifier.take() {
			drop(notifier);
		}
	}
}
