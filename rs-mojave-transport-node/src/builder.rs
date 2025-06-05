use multiaddr::PeerId;
use rs_mojave_network_core::{
	Protocol, Transport,
	muxing::StreamMuxerBox,
	transport::{self},
};
use std::collections::{HashMap, hash_map::Entry};

use crate::{Node, error::BuilderError};

pub struct Builder {
	keypair: libp2p_identity::Keypair,
	transports: HashMap<Protocol, transport::Boxed<(PeerId, StreamMuxerBox)>>,
}

impl Builder {
	pub fn new(keypair: libp2p_identity::Keypair) -> Self {
		Self {
			keypair,
			transports: HashMap::new(),
		}
	}

	pub fn with_transport(
		mut self,
		constructor: impl FnOnce(
			&libp2p_identity::Keypair,
		) -> transport::Boxed<(PeerId, StreamMuxerBox)>,
	) -> Result<Self, BuilderError> {
		let transport = constructor(&self.keypair);
		let key = transport.supported_protocols_for_dialing();

		match self.transports.entry(key) {
			Entry::Occupied(_) => Err(BuilderError::DuplicateTransport(key)),
			Entry::Vacant(entry) => {
				entry.insert(transport);
				Ok(self)
			}
		}
	}

	pub fn build(self) -> Node {
		Node::new(self.keypair.public().to_peer_id(), self.transports)
	}
}
