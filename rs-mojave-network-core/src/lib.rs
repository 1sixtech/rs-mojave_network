pub mod connection;
pub mod muxing;
pub mod transport;

pub use muxing::StreamMuxer;
pub use transport::Transport;

pub mod util {
	use std::convert::Infallible;

	/// A safe version of [`std::intrinsics::unreachable`].
	#[inline(always)]
	pub fn unreachable(x: Infallible) -> ! {
		match x {}
	}
}
