use multiaddr::PeerId;
use rs_mojave_network_core::{
	Protocol, Transport,
	muxing::StreamMuxerBox,
	transport::{self},
};
use std::collections::{HashMap, hash_map::Entry};

use crate::{Node, error::BuilderError};

pub struct BuildableStep;

pub struct ProtocolsState<TProtocols> {
	protocols: TProtocols,
}

pub struct BuildingStep;

#[derive(Default)]
pub struct BuildingState;

pub struct Builder<Step = BuildingStep, T = BuildingState> {
	keypair: libp2p_identity::Keypair,
	transports: HashMap<Protocol, transport::Boxed<(PeerId, StreamMuxerBox)>>,
	_step: std::marker::PhantomData<Step>,
	state: T,
}

impl Builder {
	pub fn new(keypair: libp2p_identity::Keypair) -> Self {
		Self {
			keypair,
			transports: HashMap::new(),
			_step: Default::default(),
			state: Default::default(),
		}
	}
}

impl Builder<BuildingStep, BuildingState> {
	pub fn with_transport(
		mut self,
		constructor: impl FnOnce(&libp2p_identity::Keypair) -> transport::Boxed<(PeerId, StreamMuxerBox)>,
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

	pub fn with_protocols<TProtocols>(
		self,
		protocols: TProtocols,
	) -> Builder<BuildableStep, ProtocolsState<TProtocols>> {
		Builder {
			keypair: self.keypair,
			transports: self.transports,
			_step: std::marker::PhantomData::<BuildableStep>,
			state: ProtocolsState { protocols },
		}
	}
}

impl<TProtocols> Builder<BuildingState, ProtocolsState<TProtocols>> {
	pub fn build(self) -> Node<TProtocols> {
		Node::new(
			self.keypair.public().to_peer_id(),
			self.state.protocols,
			self.transports,
		)
	}
}
