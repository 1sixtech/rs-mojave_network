use std::{
	pin::Pin,
	task::{Context, Poll},
	time::Duration,
};

use asynchronous_codec::{Framed, LengthCodec};
use bytes::{Buf, Bytes};
use futures::{AsyncRead, AsyncWrite, FutureExt, Sink, Stream};
use futures_timer::Delay;
use pin_project::pin_project;
use rs_mojave_network_core::muxing::SubstreamBox;
use serde::{Deserialize, Serialize};

use crate::{ProtocolHandler, StreamProtocol, protocol};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamProtocols(pub Vec<StreamProtocol>);

#[pin_project]
pub struct NegotiatorOutboundStream<S> {
	in_o_out: String,
	protocols: StreamProtocols,
	timeout: Delay,
	state: OutboundState<S>,
}

enum OutboundState<S> {
	SendProtocol {
		io: Framed<S, LengthCodec>,
	},
	Flush {
		io: Framed<S, LengthCodec>,
	},
	RecvProtocol {
		io: Framed<S, LengthCodec>,
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
			timeout: Delay::new(Duration::from_secs(15)),
			state: OutboundState::SendProtocol { io: framed },
		}
	}
}

// impl<S> Unpin for NegotiatorOutboundStream<S> {}

impl<S> Future for NegotiatorOutboundStream<S>
where
	S: AsyncWrite + AsyncRead + Unpin,
{
	type Output = Result<S, NegotiatorStreamError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		match self.timeout.poll_unpin(cx) {
			Poll::Ready(_) => return Poll::Ready(Err(NegotiatorStreamError::Timeout)),
			Poll::Pending => {}
		}

		let mut this = self.project();

		loop {
			match std::mem::replace(this.state, OutboundState::Done) {
				OutboundState::SendProtocol { mut io } => {
					match Pin::new(&mut io).poll_ready(cx) {
						Poll::Pending => {
							*this.state = OutboundState::SendProtocol { io };
							return Poll::Pending;
						}
						Poll::Ready(Ok(())) => {}
						Poll::Ready(Err(e)) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
					}

					let protocols_json = serde_json::to_vec(&this.protocols).unwrap();
					tracing::info!("Sending protocols: {:?} {}", protocols_json, this.in_o_out);
					if let Err(err) = Pin::new(&mut io).start_send(protocols_json.into()) {
						return Poll::Ready(Err(NegotiatorStreamError::IoError(err)));
					}

					*this.state = OutboundState::Flush { io };
				}

				OutboundState::Flush { mut io } => match Pin::new(&mut io).poll_flush(cx)? {
					Poll::Ready(()) => {
						*this.state = OutboundState::RecvProtocol {
							io,
							received_protocols: None,
						}
					}
					Poll::Pending => {
						*this.state = OutboundState::Flush { io };
						return Poll::Pending;
					}
				},

				OutboundState::RecvProtocol {
					mut io,
					received_protocols,
				} => {
					tracing::info!("Receiving protocols {}", this.in_o_out);
					let msg: Bytes = match Pin::new(&mut io).poll_next(cx) {
						Poll::Pending => {
							*this.state = OutboundState::RecvProtocol { io, received_protocols };
							return Poll::Pending;
						}
						Poll::Ready(None) => return Poll::Ready(Err(NegotiatorStreamError::NegotiationFailed)),
						Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
						Poll::Ready(Some(Ok(msg))) => msg,
					};
					tracing::info!("Received protocols: {:?} {}", msg, this.in_o_out);

					let received_protocols: Vec<StreamProtocol> = serde_json::from_slice(msg.as_ref()).unwrap();
					tracing::info!("Received protocols: {:?}", received_protocols);

					return Poll::Ready(Ok(io.into_inner()));
				}

				OutboundState::Done => panic!("State::poll called after completion"),
			}
		}
	}
}

#[pin_project]
pub struct NegotiatorInboundStream<S> {
	in_o_out: String,
	protocols: StreamProtocols,
	timeout: Delay,
	state: InboundState<S>,
}

enum InboundState<S> {
	RecvProtocol {
		io: Framed<S, LengthCodec>,
	},
	SendProtocol {
		io: Framed<S, LengthCodec>,
		received_protocols: Option<Vec<StreamProtocol>>,
	},
	Flush {
		io: Framed<S, LengthCodec>,
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
			timeout: Delay::new(Duration::from_secs(15)),
			state: InboundState::RecvProtocol { io: framed },
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

// impl<S> Unpin for NegotiatorInboundStream<S> {}

impl<S> Future for NegotiatorInboundStream<S>
where
	S: AsyncWrite + AsyncRead + Unpin,
{
	type Output = Result<S, NegotiatorStreamError>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		match self.timeout.poll_unpin(cx) {
			Poll::Ready(_) => return Poll::Ready(Err(NegotiatorStreamError::Timeout)),
			Poll::Pending => {}
		}

		let mut this = self.project();

		loop {
			match std::mem::replace(this.state, InboundState::Done) {
				InboundState::RecvProtocol { mut io } => {
					tracing::info!("Receiving protocols {}", this.in_o_out);
					let msg: Bytes = match Pin::new(&mut io).poll_next(cx) {
						Poll::Pending => {
							*this.state = InboundState::RecvProtocol { io };
							return Poll::Pending;
						}
						Poll::Ready(None) => return Poll::Ready(Err(NegotiatorStreamError::NegotiationFailed)),
						Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
						Poll::Ready(Some(Ok(msg))) => msg,
					};
					tracing::info!("Received protocols: {:?} {}", msg, this.in_o_out);

					let received_protocols = serde_json::from_slice(msg.as_ref()).unwrap();

					*this.state = InboundState::SendProtocol {
						io,
						received_protocols: Some(received_protocols),
					};
				}
				InboundState::SendProtocol {
					mut io,
					received_protocols,
				} => {
					match Pin::new(&mut io).poll_ready(cx) {
						Poll::Pending => {
							*this.state = InboundState::SendProtocol { io, received_protocols };
							return Poll::Pending;
						}
						Poll::Ready(Ok(())) => {}
						Poll::Ready(Err(e)) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
					}

					let protocols_json = serde_json::to_vec(&this.protocols).unwrap();
					tracing::info!("Sending protocols: {:?} {}", protocols_json, this.in_o_out);
					if let Err(err) = Pin::new(&mut io).start_send(protocols_json.into()) {
						return Poll::Ready(Err(NegotiatorStreamError::IoError(err)));
					}

					*this.state = InboundState::Flush { io, received_protocols };
				}
				InboundState::Flush {
					mut io,
					received_protocols,
				} => {
					tracing::info!("Flush {}", this.in_o_out);
					match Pin::new(&mut io).poll_flush(cx) {
						Poll::Pending => {
							*this.state = InboundState::Flush { io, received_protocols };
							return Poll::Pending;
						}
						Poll::Ready(Ok(())) => match received_protocols {
							Some(protocols) => {
								tracing::debug!(protocols=?protocols, "received protocols");
								let inner = io.into_inner();
								return Poll::Ready(Ok(inner));
							}
							None => {
								*this.state = InboundState::RecvProtocol { io };
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
