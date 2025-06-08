use std::{collections::VecDeque, convert::Infallible, default, io, iter, task::Poll, time::Duration};

use futures::{
	future::{BoxFuture, Either, FutureExt},
	prelude::*,
};
use futures_timer::Delay;
use multiaddr::PeerId;
use rs_mojave_transport_node::{
	Action, ConnectionId, FromNode, PeerProtocolBis, PeerProtocolError, ProtocolHandler, ProtocolHandlerEvent, Stream,
	StreamId, StreamProtocol, ToNode,
};

mod config;
mod error;
mod protocol;

pub use self::protocol::PROTOCOL_NAME;
pub use config::*;
pub use error::*;

pub struct Protocol {
	config: Config,

	events: VecDeque<Event>,
}

#[derive(Debug)]
pub struct Event {
	pub peer: PeerId,
	pub connection_id: ConnectionId,
	pub result: Result<Duration, Error>,
}

impl Protocol {
	pub fn new(config: Config) -> Self {
		Self {
			config,
			events: VecDeque::new(),
		}
	}
}

impl Default for Protocol {
	fn default() -> Self {
		Self::new(Config::default())
	}
}

// #[tracing::instrument(level = "debug", name = "ProtocolPing::Poll", skip(self))]
// 	fn poll(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Action<Self::Event>> {
// 		if let Some(e) = self.events.pop_back() {
// 			return Poll::Ready(Action::Event(e));
// 		}
// 		Poll::Pending
// 	}
//

enum State {
	Inactive { reported: bool },
	Active,
}

type PingFuture = BoxFuture<'static, Result<(Stream, Duration), Error>>;

/// A wrapper around [`protocol::send_ping`] that enforces a time out.
async fn send_ping(stream: Stream, timeout: Duration) -> Result<(Stream, Duration), Error> {
	let ping = crate::protocol::send_ping(stream);
	futures::pin_mut!(ping);

	match future::select(ping, Delay::new(timeout)).await {
		Either::Left((Ok((stream, rtt)), _)) => Ok((stream, rtt)),
		Either::Left((Err(e), _)) => Err(Error::Io(e)),
		Either::Right(((), _)) => Err(Error::Timeout(10)),
	}
}

type PongFuture = BoxFuture<'static, Result<Stream, io::Error>>;
pub struct PingHandler {
	state: State,
	interval: Delay,
	outbound: Option<OutboundState>,
	pong: Option<PongFuture>,
	pending_errors: VecDeque<Error>,
}

/// The current state w.r.t. outbound pings.
enum OutboundState {
	/// A new substream is being negotiated for the ping protocol.
	OpenStream,
	/// The substream is idle, waiting to send the next ping.
	Idle(Stream),
	/// A ping is being sent and the response awaited.
	Ping(PingFuture),
}

impl PingHandler {
	pub fn new() -> Self {
		Self {
			state: State::Active,
			interval: Delay::new(Duration::from_secs(1)),
			pending_errors: Default::default(),
			outbound: None,
			pong: None,
		}
	}
}

impl Default for PingHandler {
	fn default() -> Self {
		Self::new()
	}
}

impl ProtocolHandler for PingHandler {
	type FromProtocol = Infallible;
	type ToProtocol = Result<Duration, Error>;
	type ProtocolInfoIter = iter::Once<StreamProtocol>;

	fn protocol_info(&self) -> Self::ProtocolInfoIter {
		iter::once(PROTOCOL_NAME.clone())
	}

	fn on_protocol_event(&mut self, event: Self::FromProtocol) {
		todo!()
	}

	fn poll(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<ProtocolHandlerEvent<Self::ToProtocol>> {
		match self.state {
			State::Inactive { .. } => {
				// TODO
			}
			State::Active => {}
		}

		// did we receive a pong?
		if let Some(fut) = self.pong.as_mut() {
			match fut.poll_unpin(cx) {
				Poll::Pending => {}
				Poll::Ready(Ok(stream)) => self.pong = Some(protocol::recv_ping(stream).boxed()),
				Poll::Ready(Err(err)) => {
					self.pong = None;
					tracing::error!("PingHandler::poll: {:?}", err);
				}
			}
		}

		loop {
			match self.outbound.take() {
				Some(state) => match state {
					OutboundState::OpenStream => {
						self.outbound = Some(OutboundState::OpenStream);
						break;
					}
					OutboundState::Idle(stream) => match self.interval.poll_unpin(cx) {
						Poll::Ready(_) => {
							self.outbound =
								Some(OutboundState::Ping(send_ping(stream, Duration::from_secs(10)).boxed()));
						}
						Poll::Pending => {
							self.outbound = Some(OutboundState::Idle(stream));
							break;
						}
					},
					OutboundState::Ping(mut ping) => match ping.poll_unpin(cx) {
						Poll::Ready(e) => match e {
							Ok((stream, rtt)) => {
								tracing::info!("PingHandler::poll: {:?}", rtt);
								self.interval.reset(Duration::from_secs(1));
								self.outbound = Some(OutboundState::Idle(stream));
								return Poll::Ready(ProtocolHandlerEvent::NotifyProtocol(Ok(rtt)));
							}
							Err(e) => {
								self.interval.reset(Duration::from_secs(1));
								self.pending_errors.push_front(e);
							}
						},
						Poll::Pending => {
							self.outbound = Some(OutboundState::Ping(ping));
							break;
						}
					},
				},
				None => match self.interval.poll_unpin(cx) {
					Poll::Ready(_) => {
						self.outbound = Some(OutboundState::OpenStream);
						return Poll::Ready(ProtocolHandlerEvent::OutboundSubstreamRequest);
					}
					Poll::Pending => break,
				},
			}
		}

		Poll::Pending
	}
}

impl PeerProtocolBis for Protocol {
	type ToNode = Event;

	type Handler = PingHandler;

	#[tracing::instrument(level = "debug", name = "ProtocolPing::OnNewConnection", skip(self))]
	fn on_new_connection(
		&mut self,
		connection_id: ConnectionId,
		peer_id: PeerId,
		remote_addr: &multiaddr::Multiaddr,
		local_addr: Option<&multiaddr::Multiaddr>,
	) -> Result<Self::Handler, rs_mojave_transport_node::ConnectionError> {
		Ok(PingHandler::new())
	}

	fn on_node_event(&mut self, event: FromNode) {
		todo!()
	}

	fn poll(
		&mut self,
		cx: &mut std::task::Context<'_>,
	) -> Poll<Action<Self::ToNode, rs_mojave_transport_node::THandlerFromEvent<Self>>> {
		todo!()
	}
}
