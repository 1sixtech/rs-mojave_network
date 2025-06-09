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
pub struct OutboundStream<S> {
	timeout: Delay,
	protocols: StreamProtocols,
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

impl<S> OutboundStream<S>
where
	S: AsyncWrite + AsyncRead + Unpin,
{
	pub(crate) fn new(protocols: StreamProtocols, stream: S) -> Self {
		let framed = Framed::new(stream, LengthCodec);

		OutboundStream {
			protocols,
			timeout: Delay::new(Duration::from_secs(15)),
			state: OutboundState::SendProtocol { io: framed },
		}
	}
}

impl<S> Future for OutboundStream<S>
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
					let msg: Bytes = match Pin::new(&mut io).poll_next(cx) {
						Poll::Pending => {
							*this.state = OutboundState::RecvProtocol { io, received_protocols };
							return Poll::Pending;
						}
						Poll::Ready(None) => return Poll::Ready(Err(NegotiatorStreamError::NegotiationFailed)),
						Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(NegotiatorStreamError::IoError(e))),
						Poll::Ready(Some(Ok(msg))) => msg,
					};

					let received_protocols: Vec<StreamProtocol> = serde_json::from_slice(msg.as_ref()).unwrap();
					tracing::info!("Received protocols: {:?}", received_protocols);

					return Poll::Ready(Ok(io.into_inner()));
				}

				OutboundState::Done => panic!("State::poll called after completion"),
			}
		}
	}
}
