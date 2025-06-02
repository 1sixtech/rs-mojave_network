use multiaddr::{Multiaddr, PeerId};
use std::io;

use crate::connection::ConnectionId;

#[derive(Debug)]
pub enum NodeEvent {
	ConnectionEstablished {
		connection_id: ConnectionId,
		peer_id: PeerId,
	},
	IncomingConnection {
		remote_address: Multiaddr,
	},
	NewListenAddr {
		address: Multiaddr,
	},
	AddressExpired {
		address: Multiaddr,
	},
	ListenerClosed {
		reason: Result<(), io::Error>,
	},
	ListenerError {
		error: io::Error,
	},
}
