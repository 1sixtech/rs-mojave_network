use multiaddr::PeerId;
use rs_mojave_network_core::{
	Transport,
	muxing::StreamMuxerBox,
	transport::{self, Protocol},
};
use std::collections::{HashMap, hash_map::Entry};

use crate::{Node, PeerProtocol, error::BuilderError};

pub struct BuildableStep;

pub struct ProtocolsState<TProtocols>
where
	TProtocols: PeerProtocol,
{
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

	pub fn with_protocol<P: PeerProtocol, R: TryIntoPeerProtocol<P> + PeerProtocol>(
		self,
		constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
	) -> Result<Builder<BuildableStep, ProtocolsState<P>>, R::Error> {
		let protocols = constructor(&self.keypair).try_into_peer_protocol()?;

		Ok(Builder {
			keypair: self.keypair,
			transports: self.transports,
			_step: std::marker::PhantomData::<BuildableStep>,
			state: ProtocolsState { protocols },
		})
	}
}

impl<TProtocols> Builder<BuildableStep, ProtocolsState<TProtocols>>
where
	TProtocols: PeerProtocol,
{
	pub fn build(self) -> Node<TProtocols> {
		Node::new(
			self.keypair.public().to_peer_id(),
			self.state.protocols,
			self.transports,
		)
	}
}

pub trait TryIntoPeerProtocol<P> {
	type Error;

	fn try_into_peer_protocol(self) -> Result<P, Self::Error>;
}

impl<P> TryIntoPeerProtocol<P> for P
where
	P: PeerProtocol,
{
	type Error = std::convert::Infallible;

	fn try_into_peer_protocol(self) -> Result<P, Self::Error> {
		Ok(self)
	}
}

#[derive(Debug, thiserror::Error)]
#[error("failed to build peer protocol: {0}")]
pub struct PeerProtocolBuildError(Box<dyn std::error::Error + Send + Sync + 'static>);

impl<P> TryIntoPeerProtocol<P> for Result<P, Box<dyn std::error::Error + Send + Sync>>
where
	P: PeerProtocol,
{
	type Error = PeerProtocolBuildError;

	fn try_into_peer_protocol(self) -> Result<P, Self::Error> {
		self.map_err(PeerProtocolBuildError)
	}
}
