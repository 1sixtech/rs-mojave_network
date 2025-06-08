use std::{
	fmt::{Debug, Display},
	sync::LazyLock,
};

use parking_lot::Mutex;
use slab::Slab;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

static STREAM_ID_ALLOCATOR: LazyLock<Mutex<Slab<()>>> = LazyLock::new(|| Mutex::new(Slab::new()));

impl StreamId {
	pub fn next() -> Self {
		let mut slab = STREAM_ID_ALLOCATOR.lock();
		StreamId(slab.insert(()))
	}

	pub fn remove(self) {
		let mut slab = STREAM_ID_ALLOCATOR.lock();
		slab.remove(self.0);
	}
}

impl Debug for StreamId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "StreamId({})", self.0)
	}
}

impl Display for StreamId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}

macro_rules! impl_connection_id_from_unsigned {
    ($($t:ty),*) => {
        $(
            impl From<$t> for StreamId {
                fn from(val: $t) -> Self {
                    if val as u64 > usize::MAX as u64 {
                        panic!("Value too large for usize");
                    }
                    StreamId(val as usize)
                }
            }
        )*
    };
}

macro_rules! impl_connection_id_from_signed {
    ($($t:ty),*) => {
        $(
            impl From<$t> for StreamId {
                fn from(val: $t) -> Self {
                    if val < 0 {
                        panic!("Negative value cannot be converted to StreamId");
                    }
                    if val as u64 > usize::MAX as u64 {
                        panic!("Value too large for usize");
                    }
                    StreamId(val as usize)
                }
            }
        )*
    };
}

impl_connection_id_from_unsigned!(u8, u16, u32, u64, usize);
impl_connection_id_from_signed!(i8, i16, i32, i64, isize);
