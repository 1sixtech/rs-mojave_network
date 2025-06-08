use std::{
	pin::Pin,
	task::{Context, Poll},
	time::Duration,
};

use asynchronous_codec::{Framed, LengthCodec};
use bytes::{Buf, Bytes};
use futures::{AsyncRead, AsyncWrite, FutureExt, Sink, Stream};
use futures_timer::Delay;
use serde::{Deserialize, Serialize};

use crate::{ProtocolHandler, StreamProtocol, protocol};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamProtocols(pub Vec<StreamProtocol>);

pub struct NegotiatorOutboundStream<S> {
	in_o_out: String,
	protocols: StreamProtocols,
	timeout: Delay,
	io: Framed<S, LengthCodec>,
	state: OutboundState,
}

enum OutboundState {
	SendProtocol,
	Flush,
	RecvProtocol {
		received_protocols: Option<Vec<StreamProtocol>>,
	},
	Done,
}

impl<S> NegotiatorOutboundStream<S>
where
	S: AsyncWrite + AsyncRead + Unpin,
{
	pub(crate) fn new(protocols: StreamProtocols, stream: S) -> Self {
		let framed = Framed::new(stream, LengthCodec);

		NegotiatorOutboundStream {
			in_o_out: "in".to_string(),
			protocols,
			io: framed,
			timeout: Delay::new(Duration::from_secs(15)),
			state: OutboundState::SendProtocol,
		}
	}
}

impl<S> Unpin for NegotiatorOutboundStream<S> {}

impl<S> Future for NegotiatorOutboundStream<S>
where
	S: AsyncWrite + AsyncRead + Unpin,
{
	type Output = Result<(), NegotiatorStreamError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		match self.timeout.poll_unpin(cx) {
			Poll::Ready(_) => return Poll::Ready(Err(NegotiatorStreamError::Timeout)),
			Poll::Pending => {}
		}

		loop {
			match std::mem::replace(&mut self.state, OutboundState::Done) {
				OutboundState::SendProtocol => {
					match Pin::new(&mut self.io).poll_ready(cx) {
						Poll::Pending => {
							self.state = OutboundState::SendProtocol;
							return Poll::Pending;
						}
						Poll::Ready(Ok(())) => {}
						Poll::Ready(Err(e)) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
					}

					let protocols_json = serde_json::to_vec(&self.protocols).unwrap();
					tracing::info!("Sending protocols: {:?} {}", protocols_json, self.in_o_out);
					if let Err(err) = Pin::new(&mut self.io).start_send(protocols_json.into()) {
						return Poll::Ready(Err(NegotiatorStreamError::IoError(err)));
					}

					self.state = OutboundState::Flush;
				}

				OutboundState::Flush => match Pin::new(&mut self.io).poll_flush(cx)? {
					Poll::Ready(()) => {
						self.state = OutboundState::RecvProtocol {
							received_protocols: None,
						}
					}
					Poll::Pending => {
						self.state = OutboundState::Flush;
						return Poll::Pending;
					}
				},

				OutboundState::RecvProtocol { received_protocols } => {
					tracing::info!("Receiving protocols {}", self.in_o_out);
					let msg: Bytes = match Pin::new(&mut self.io).poll_next(cx) {
						Poll::Pending => {
							self.state = OutboundState::RecvProtocol { received_protocols };
							return Poll::Pending;
						}
						Poll::Ready(None) => return Poll::Ready(Err(NegotiatorStreamError::NegotiationFailed)),
						Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
						Poll::Ready(Some(Ok(msg))) => msg,
					};
					tracing::info!("Received protocols: {:?} {}", msg, self.in_o_out);

					let received_protocols: Vec<StreamProtocol> = serde_json::from_slice(msg.as_ref()).unwrap();
					tracing::info!("Received protocols: {:?}", received_protocols);

					return Poll::Ready(Ok(()));
				}

				OutboundState::Done => panic!("State::poll called after completion"),
			}
		}
	}
}

pub struct NegotiatorInboundStream<S> {
	in_o_out: String,
	protocols: StreamProtocols,
	timeout: Delay,
	io: Framed<S, LengthCodec>,
	state: InboundState,
}

enum InboundState {
	RecvProtocol,
	SendProtocol {
		received_protocols: Option<Vec<StreamProtocol>>,
	},
	Flush {
		received_protocols: Option<Vec<StreamProtocol>>,
	},
	Done,
}

impl<S> NegotiatorInboundStream<S>
where
	S: AsyncWrite + AsyncRead + Unpin,
{
	pub(crate) fn new(protocols: StreamProtocols, stream: S) -> Self {
		let framed = Framed::new(stream, LengthCodec);

		NegotiatorInboundStream {
			in_o_out: "in".to_string(),
			protocols,
			io: framed,
			timeout: Delay::new(Duration::from_secs(15)),
			state: InboundState::RecvProtocol,
		}
	}
}

#[derive(Debug, thiserror::Error)]
pub enum NegotiatorStreamError {
	#[error("Timeout while negotiating timeout")]
	Timeout,

	#[error("I/O error while negotiating")]
	IoError(#[from] std::io::Error),

	#[error("Negotiation failed")]
	NegotiationFailed,

	#[error("Invalid protocol")]
	InvalidProtocol,
}

impl<S> Unpin for NegotiatorInboundStream<S> {}

impl<S> Future for NegotiatorInboundStream<S>
where
	S: AsyncWrite + AsyncRead + Unpin,
{
	type Output = Result<(), NegotiatorStreamError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		match self.timeout.poll_unpin(cx) {
			Poll::Ready(_) => return Poll::Ready(Err(NegotiatorStreamError::Timeout)),
			Poll::Pending => {}
		}

		loop {
			match std::mem::replace(&mut self.state, InboundState::Done) {
				InboundState::RecvProtocol => {
					tracing::info!("Receiving protocols {}", self.in_o_out);
					let msg: Bytes = match Pin::new(&mut self.io).poll_next(cx) {
						Poll::Pending => {
							self.state = InboundState::RecvProtocol;
							return Poll::Pending;
						}
						Poll::Ready(None) => return Poll::Ready(Err(NegotiatorStreamError::NegotiationFailed)),
						Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
						Poll::Ready(Some(Ok(msg))) => msg,
					};
					tracing::info!("Received protocols: {:?} {}", msg, self.in_o_out);

					let received_protocols = serde_json::from_slice(msg.as_ref()).unwrap();

					self.state = InboundState::SendProtocol {
						received_protocols: Some(received_protocols),
					};
				}
				InboundState::SendProtocol { received_protocols } => {
					match Pin::new(&mut self.io).poll_ready(cx) {
						Poll::Pending => {
							self.state = InboundState::SendProtocol { received_protocols };
							return Poll::Pending;
						}
						Poll::Ready(Ok(())) => {}
						Poll::Ready(Err(e)) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
					}

					let protocols_json = serde_json::to_vec(&self.protocols).unwrap();
					tracing::info!("Sending protocols: {:?} {}", protocols_json, self.in_o_out);
					if let Err(err) = Pin::new(&mut self.io).start_send(protocols_json.into()) {
						return Poll::Ready(Err(NegotiatorStreamError::IoError(err)));
					}

					self.state = InboundState::Flush { received_protocols };
				}
				InboundState::Flush { received_protocols } => {
					tracing::info!("Flush {}", self.in_o_out);
					match Pin::new(&mut self.io).poll_flush(cx) {
						Poll::Pending => {
							self.state = InboundState::Flush { received_protocols };
							return Poll::Pending;
						}
						Poll::Ready(Ok(())) => match received_protocols {
							Some(protocols) => {
								tracing::debug!(protocols=?protocols, "received protocols");
								return Poll::Ready(Ok(()));
							}
							None => {
								self.state = InboundState::RecvProtocol;
							}
						},
						Poll::Ready(Err(e)) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
					}
				}
				InboundState::Done => panic!("NegotiatorStream is in a done state"),
			}
		}
	}
}
