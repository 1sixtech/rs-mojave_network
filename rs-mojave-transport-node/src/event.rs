use multiaddr::Multiaddr;
use std::io;

#[derive(Debug)]
pub enum NodeEvent {
	IncomingConnection { remote_address: Multiaddr },
	NewListenAddr { address: Multiaddr },
	AddressExpired { address: Multiaddr },
	ListenerClosed { reason: Result<(), io::Error> },
	ListenerError { error: io::Error },
}
