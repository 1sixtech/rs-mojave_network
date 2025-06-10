use std::{collections::VecDeque, task::Poll, time::Duration};

use multiaddr::PeerId;
use rs_mojave_transport_node::{Action, ConnectionId, FromNode, PeerProtocol};

mod config;
mod error;
mod handler;
mod protocol;

use crate::handler::Handler;

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

impl PeerProtocol for Protocol {
	type ToNode = Event;

	type Handler = Handler;

	#[tracing::instrument(level = "debug", name = "ProtocolPing::OnNewConnection", skip(self))]
	fn on_new_connection(
		&mut self,
		connection_id: ConnectionId,
		peer_id: PeerId,
		remote_addr: &multiaddr::Multiaddr,
		local_addr: Option<&multiaddr::Multiaddr>,
	) -> Result<Self::Handler, rs_mojave_transport_node::ConnectionError> {
		Ok(Handler::new(self.config.clone()))
	}

	fn on_node_event(&mut self, _: FromNode) {}

	fn poll(
		&mut self,
		_: &mut std::task::Context<'_>,
	) -> Poll<Action<Self::ToNode, rs_mojave_transport_node::THandlerFromEvent<Self>>> {
		if let Some(event) = self.events.pop_back() {
			return Poll::Ready(Action::Event(event));
		}
		Poll::Pending
	}
}
