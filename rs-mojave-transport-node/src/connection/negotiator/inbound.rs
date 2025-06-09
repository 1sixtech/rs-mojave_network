use std::{
	pin::Pin,
	task::{Context, Poll},
	time::Duration,
};

use asynchronous_codec::{Framed, LengthCodec};
use bytes::Bytes;
use futures::{AsyncRead, AsyncWrite, FutureExt, Sink, Stream};
use futures_timer::Delay;
use pin_project::pin_project;

use crate::{
	StreamProtocol,
	connection::negotiator::{NegotiatorStreamError, StreamProtocols},
};

#[pin_project]
pub struct InboundStream<S> {
	timeout: Delay,
	protocols: StreamProtocols,
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

impl<S> InboundStream<S>
where
	S: AsyncWrite + AsyncRead + Unpin,
{
	pub(crate) fn new(protocols: StreamProtocols, stream: S) -> Self {
		let framed = Framed::new(stream, LengthCodec);
		InboundStream {
			protocols,
			timeout: Delay::new(Duration::from_secs(15)),
			state: InboundState::RecvProtocol { io: framed },
		}
	}
}

impl<S> Future for InboundStream<S>
where
	S: AsyncWrite + AsyncRead + Unpin,
{
	type Output = Result<S, NegotiatorStreamError>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		let this = self.project();

		if this.timeout.poll_unpin(cx).is_ready() {
			return Poll::Ready(Err(NegotiatorStreamError::Timeout));
		}

		loop {
			match std::mem::replace(this.state, InboundState::Done) {
				InboundState::RecvProtocol { mut io } => {
					let msg: Bytes = match Pin::new(&mut io).poll_next(cx) {
						Poll::Pending => {
							*this.state = InboundState::RecvProtocol { io };
							return Poll::Pending;
						}
						Poll::Ready(None) => return Poll::Ready(Err(NegotiatorStreamError::NegotiationFailed)),
						Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
						Poll::Ready(Some(Ok(msg))) => msg,
					};

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
					if let Err(err) = Pin::new(&mut io).start_send(protocols_json.into()) {
						return Poll::Ready(Err(NegotiatorStreamError::IoError(err)));
					}

					*this.state = InboundState::Flush { io, received_protocols };
				}
				InboundState::Flush {
					mut io,
					received_protocols,
				} => match Pin::new(&mut io).poll_flush(cx) {
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
				},
				InboundState::Done => panic!("NegotiatorStream is in a done state"),
			}
		}
	}
}
