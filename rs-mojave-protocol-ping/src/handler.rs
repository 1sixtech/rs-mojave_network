use std::{collections::VecDeque, convert::Infallible, io, iter, task::Poll, time::Duration};

use futures::{
	future::{BoxFuture, Either, FutureExt},
	prelude::*,
};
use futures_timer::Delay;
use rs_mojave_transport_node::{
	AsyncReadWrite, ConnectionId, FromNode, PeerProtocol, ProtocolHandler, ProtocolHandlerEvent, StreamProtocol,
};

use crate::{Config, Error, PROTOCOL_NAME, protocol};

type PingFuture = BoxFuture<'static, Result<(Box<dyn AsyncReadWrite + Send + Unpin>, Duration), Error>>;

enum State {
	Inactive { reported: bool },
	Active,
}

/// The current state w.r.t. outbound pings.
enum OutboundState {
	/// A new substream is being negotiated for the ping protocol.
	OpenStream,
	/// The substream is idle, waiting to send the next ping.
	Idle(Box<dyn AsyncReadWrite + Send + Unpin>),
	/// A ping is being sent and the response awaited.
	Ping(PingFuture),
}

/// A wrapper around [`protocol::send_ping`] that enforces a time out.
async fn send_ping(
	stream: Box<dyn AsyncReadWrite + Send + Unpin>,
	timeout: Duration,
) -> Result<(Box<dyn AsyncReadWrite + Send + Unpin>, Duration), Error> {
	let ping = crate::protocol::send_ping(stream);
	futures::pin_mut!(ping);

	match future::select(ping, Delay::new(timeout)).await {
		Either::Left((Ok((stream, rtt)), _)) => Ok((stream, rtt)),
		Either::Left((Err(e), _)) => Err(Error::Io(e)),
		Either::Right(((), _)) => Err(Error::Timeout(10)),
	}
}

type PongFuture = BoxFuture<'static, Result<Box<dyn AsyncReadWrite + Send + Unpin>, io::Error>>;
pub struct Handler {
	config: Config,
	state: State,
	interval: Delay,
	outbound: Option<OutboundState>,
	pong: Option<PongFuture>,
	pending_errors: VecDeque<Error>,
}

impl Handler {
	pub fn new(config: Config) -> Self {
		Self {
			config,
			state: State::Active,
			interval: Delay::new(Duration::new(0, 0)),
			pending_errors: Default::default(),
			outbound: None,
			pong: None,
		}
	}
}

impl ProtocolHandler for Handler {
	type FromProtocol = Infallible;
	type ToProtocol = Result<Duration, Error>;
	type ProtocolInfoIter = iter::Once<StreamProtocol>;

	fn protocol_info(&self) -> Self::ProtocolInfoIter {
		iter::once(PROTOCOL_NAME.clone())
	}

	fn on_protocol_event(&mut self, _: Self::FromProtocol) {}

	fn on_connection_event(&mut self, event: rs_mojave_transport_node::ConnectionEvent) {
		match event {
			rs_mojave_transport_node::ConnectionEvent::NewInboundStream(substream_box) => {
				self.pong = Some(protocol::recv_ping(substream_box).boxed());
			}
			rs_mojave_transport_node::ConnectionEvent::NewOutboundStream(substream_box) => {
				self.outbound = Some(OutboundState::Ping(
					send_ping(substream_box, self.config.timeout).boxed(),
				));
			}
			rs_mojave_transport_node::ConnectionEvent::FailNegotiation(err) => {
				self.outbound = None;

				self.interval.reset(Duration::new(0, 0));
				let error = match err {
					rs_mojave_transport_node::negotiator::NegotiatorStreamError::Timeout => Error::Io(
						std::io::Error::new(std::io::ErrorKind::TimedOut, "ping protocol negotiation timed out"),
					),
					rs_mojave_transport_node::negotiator::NegotiatorStreamError::IoError(error) => Error::Io(error),
					rs_mojave_transport_node::negotiator::NegotiatorStreamError::NegotiationFailed => {
						self.state = State::Inactive { reported: false };
						return;
					}
				};

				self.pending_errors.push_front(error);
			}
		}
	}

	#[tracing::instrument(level = "debug", name = "PingHandler::poll", skip(cx, self))]
	fn poll(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<ProtocolHandlerEvent<Self::ToProtocol>> {
		match self.state {
			State::Inactive { reported: true } => return Poll::Pending,
			State::Inactive { reported: false } => {
				self.state = State::Inactive { reported: true };
				return Poll::Ready(ProtocolHandlerEvent::NotifyProtocol(Err(Error::UnsupportedProtocol)));
			}
			State::Active => {}
		}

		// did we receive a pong?
		if let Some(fut) = self.pong.as_mut() {
			match fut.poll_unpin(cx) {
				Poll::Pending => {}
				Poll::Ready(Ok(stream)) => self.pong = Some(protocol::recv_ping(stream).boxed()),
				Poll::Ready(Err(err)) => {
					tracing::error!("Handler::poll: {:?}", err);
					self.pong = None;
				}
			}
		}

		loop {
			if let Some(error) = self.pending_errors.pop_front() {
				tracing::error!("Handler::poll: {:?}", error);
				return Poll::Ready(ProtocolHandlerEvent::NotifyProtocol(Err(error)));
			}

			match self.outbound.take() {
				Some(state) => match state {
					OutboundState::OpenStream => {
						self.outbound = Some(OutboundState::OpenStream);
						break;
					}

					OutboundState::Idle(stream) => match self.interval.poll_unpin(cx) {
						Poll::Ready(_) => {
							self.outbound = Some(OutboundState::Ping(send_ping(stream, self.config.timeout).boxed()));
						}
						Poll::Pending => {
							self.outbound = Some(OutboundState::Idle(stream));
							break;
						}
					},

					OutboundState::Ping(mut ping) => match ping.poll_unpin(cx) {
						Poll::Ready(e) => match e {
							Ok((stream, rtt)) => {
								tracing::info!(?rtt, "PingHandler::ping succeeded");
								self.interval.reset(self.config.interval);
								self.outbound = Some(OutboundState::Idle(stream));
								return Poll::Ready(ProtocolHandlerEvent::NotifyProtocol(Ok(rtt)));
							}
							Err(e) => {
								self.interval.reset(self.config.interval);
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
