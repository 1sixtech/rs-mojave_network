use parking_lot::Mutex;
use slab::Slab;
use std::{
	fmt::{Debug, Display},
	sync::LazyLock,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(usize);

static CONNECTION_ID_ALLOCATOR: LazyLock<Mutex<Slab<()>>> = LazyLock::new(|| Mutex::new(Slab::new()));

impl ConnectionId {
	pub fn next() -> Self {
		let mut slab = CONNECTION_ID_ALLOCATOR.lock();
		ConnectionId(slab.insert(()))
	}

	pub fn remove(self) {
		let mut slab = CONNECTION_ID_ALLOCATOR.lock();
		slab.remove(self.0);
	}
}

impl Debug for ConnectionId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "ConnectionId({})", self.0)
	}
}

impl Display for ConnectionId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}

macro_rules! impl_connection_id_from_unsigned {
    ($($t:ty),*) => {
        $(
            impl From<$t> for ConnectionId {
                fn from(val: $t) -> Self {
                    if val as u64 > usize::MAX as u64 {
                        panic!("Value too large for usize");
                    }
                    ConnectionId(val as usize)
                }
            }
        )*
    };
}

macro_rules! impl_connection_id_from_signed {
    ($($t:ty),*) => {
        $(
            impl From<$t> for ConnectionId {
                fn from(val: $t) -> Self {
                    if val < 0 {
                        panic!("Negative value cannot be converted to ConnectionId");
                    }
                    if val as u64 > usize::MAX as u64 {
                        panic!("Value too large for usize");
                    }
                    ConnectionId(val as usize)
                }
            }
        )*
    };
}

impl_connection_id_from_unsigned!(u8, u16, u32, u64, usize);
impl_connection_id_from_signed!(i8, i16, i32, i64, isize);

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::HashMap;

	#[test]
	fn test_connection_id_creation() {
		let id = ConnectionId::from(42usize);
		assert_eq!(format!("{id:?}"), "ConnectionId(42)");
	}

	#[test]
	fn test_connection_id_from_various_types() {
		let from_usize = ConnectionId::from(123usize);
		let from_u32 = ConnectionId::from(123u32);
		let from_u64 = ConnectionId::from(123u64);

		assert_eq!(from_usize, from_u32);
		assert_eq!(from_u32, from_u64);
		assert_eq!(format!("{from_usize:?}"), "ConnectionId(123)");
	}

	#[test]
	fn test_connection_id_equality() {
		let id1 = ConnectionId::from(100);
		let id2 = ConnectionId::from(100);
		let id3 = ConnectionId::from(200);

		assert_eq!(id1, id2);
		assert_ne!(id1, id3);
		assert_ne!(id2, id3);
	}

	#[test]
	fn test_connection_id_clone() {
		let id1 = ConnectionId::from(42);
		let id2 = id1;

		assert_eq!(id1, id2);
		assert_eq!(format!("{id1:?}"), format!("{:?}", id2));
	}

	#[test]
	fn test_connection_id_copy() {
		let id1 = ConnectionId::from(42);
		let id2 = id1; // Copy should work

		assert_eq!(id1, id2);
		// Both should still be usable after copy
		assert_eq!(format!("{id1:?}"), "ConnectionId(42)");
		assert_eq!(format!("{id2:?}"), "ConnectionId(42)");
	}

	#[test]
	fn test_connection_id_hash() {
		use std::collections::hash_map::DefaultHasher;
		use std::hash::{Hash, Hasher};

		let id1 = ConnectionId::from(42);
		let id2 = ConnectionId::from(42);
		let id3 = ConnectionId::from(43);

		let mut hasher1 = DefaultHasher::new();
		let mut hasher2 = DefaultHasher::new();
		let mut hasher3 = DefaultHasher::new();

		id1.hash(&mut hasher1);
		id2.hash(&mut hasher2);
		id3.hash(&mut hasher3);

		assert_eq!(hasher1.finish(), hasher2.finish());
		assert_ne!(hasher1.finish(), hasher3.finish());
	}

	#[test]
	fn test_connection_id_in_hashmap() {
		let mut map = HashMap::new();
		let id1 = ConnectionId::from(1);
		let id2 = ConnectionId::from(2);
		let id3 = ConnectionId::from(1); // Same as id1

		map.insert(id1, "first");
		map.insert(id2, "second");
		map.insert(id3, "first_updated"); // Should update the value for id1

		assert_eq!(map.len(), 2);
		assert_eq!(map.get(&id1), Some(&"first_updated"));
		assert_eq!(map.get(&id2), Some(&"second"));
		assert_eq!(map.get(&id3), Some(&"first_updated"));
	}

	#[test]
	fn test_connection_id_debug_format() {
		let id_zero = ConnectionId::from(0);
		let id_max = ConnectionId::from(usize::MAX);
		let id_random = ConnectionId::from(12345);

		assert_eq!(format!("{id_zero:?}"), "ConnectionId(0)");
		assert_eq!(format!("{id_max:?}"), format!("ConnectionId({})", usize::MAX));
		assert_eq!(format!("{id_random:?}"), "ConnectionId(12345)");
	}

	#[test]
	fn test_connection_id_edge_cases() {
		let id_zero = ConnectionId::from(0);
		let id_max = ConnectionId::from(usize::MAX);

		assert_eq!(format!("{id_zero:?}"), "ConnectionId(0)");
		assert_eq!(format!("{id_max:?}"), format!("ConnectionId({})", usize::MAX));
		assert_ne!(id_zero, id_max);
	}

	#[test]
	fn test_connection_id_next() {
		let id1 = ConnectionId::next();
		let id2 = ConnectionId::next();
		let id3 = ConnectionId::next();

		// Each call to next() should return a unique ID
		assert_ne!(id1, id2);
		assert_ne!(id2, id3);
		assert_ne!(id1, id3);

		// IDs should be valid debug representations
		assert!(format!("{id1:?}").starts_with("ConnectionId("));
		assert!(format!("{id2:?}").starts_with("ConnectionId("));
		assert!(format!("{id3:?}").starts_with("ConnectionId("));
	}

	#[test]
	fn test_connection_id_remove() {
		let id = ConnectionId::next();

		// Remove should succeed
		id.remove();
	}

	#[test]
	fn test_connection_id_next_and_remove_cycle() {
		// Allocate some IDs
		let id1 = ConnectionId::next();
		let id2 = ConnectionId::next();
		let id3 = ConnectionId::next();

		// All should be different
		assert_ne!(id1, id2);
		assert_ne!(id2, id3);
		assert_ne!(id1, id3);

		// Remove one of them
		id2.remove();

		// Allocate more IDs
		let id4 = ConnectionId::next();
		let id5 = ConnectionId::next();

		// New IDs should be different from existing ones
		assert_ne!(id4, id1);
		assert_ne!(id4, id3);
		assert_ne!(id5, id1);
		assert_ne!(id5, id3);
		assert_ne!(id4, id5);

		// Clean up
		id1.remove();
		id3.remove();
		id4.remove();
		id5.remove();
	}

	#[test]
	fn test_connection_id_reuse_after_remove() {
		// Allocate and immediately remove an ID
		let id1 = ConnectionId::next();
		id1.remove();

		// The slab may reuse the slot, so let's allocate a few more
		let id2 = ConnectionId::next();
		let id3 = ConnectionId::next();

		// They should be valid IDs
		assert!(format!("{id2:?}").starts_with("ConnectionId("));
		assert!(format!("{id3:?}").starts_with("ConnectionId("));
		assert_ne!(id2, id3);

		// Clean up
		id2.remove();
		id3.remove();
	}

	#[test]
	fn test_connection_id_multiple_allocations() {
		let mut ids = Vec::new();

		// Allocate multiple IDs
		for _ in 0..10 {
			let id = ConnectionId::next();
			ids.push(id);
		}

		// All IDs should be unique
		for (i, &id1) in ids.iter().enumerate() {
			for (j, &id2) in ids.iter().enumerate() {
				if i != j {
					assert_ne!(id1, id2, "IDs at positions {i} and {j} should be different");
				}
			}
		}

		// Remove all IDs
		for id in ids {
			id.remove();
		}
	}

	#[test]
	fn test_connection_id_multiple_next_calls() {
		// Test allocating multiple IDs and ensure they're all unique
		let mut allocated_ids = Vec::new();

		// Allocate 5 IDs
		for _ in 0..5 {
			let id = ConnectionId::next();
			allocated_ids.push(id);
		}

		// Verify all are unique
		for i in 0..allocated_ids.len() {
			for j in (i + 1)..allocated_ids.len() {
				assert_ne!(allocated_ids[i], allocated_ids[j]);
			}
		}

		// Clean up all IDs
		for id in allocated_ids {
			id.remove();
		}
	}

	#[test]
	fn test_connection_id_zero_value() {
		// Test that we can handle a ConnectionId with value 0
		let zero_id = ConnectionId::from(0usize);
		assert_eq!(format!("{zero_id:?}"), "ConnectionId(0)");
	}

	#[test]
	fn test_connection_id_allocator_consistency() {
		// Test that the allocator maintains consistency across multiple operations
		let id1 = ConnectionId::next();
		let id2 = ConnectionId::next();

		// IDs should be different
		assert_ne!(id1, id2);

		// Remove first ID
		id1.remove();

		// Allocate another ID
		let id3 = ConnectionId::next();

		// New ID should be different from the one still allocated
		assert_ne!(id2, id3);

		// Clean up
		id2.remove();
		id3.remove();
	}

	#[test]
	fn test_connection_id_remove_non_allocated() {
		// Create a ConnectionId without using the allocator
		let fake_id = ConnectionId::from(999999usize);

		// Removing a non-allocated ID should not panic with parking_lot mutex
		// but slab.remove() will still panic on invalid indices
		// This is expected behavior - we're testing that the mutex itself doesn't cause issues
		std::panic::catch_unwind(|| {
			fake_id.remove();
		})
		.expect_err("Should panic when trying to remove non-allocated ID");
	}

	#[test]
	fn test_connection_id_stress_allocation() {
		// Test allocating and deallocating many IDs to ensure robustness
		let mut allocated_ids = Vec::new();

		// Allocate 50 IDs
		for _ in 0..50 {
			let id = ConnectionId::next();
			allocated_ids.push(id);
		}

		// Verify all are unique
		for i in 0..allocated_ids.len() {
			for j in (i + 1)..allocated_ids.len() {
				assert_ne!(allocated_ids[i], allocated_ids[j]);
			}
		}

		// Remove every other ID
		for (i, &id) in allocated_ids.iter().enumerate() {
			if i % 2 == 0 {
				id.remove();
			}
		}

		// Allocate 25 more IDs (should reuse some slots)
		let mut new_ids = Vec::new();
		for _ in 0..25 {
			let id = ConnectionId::next();
			new_ids.push(id);
		}

		// Clean up remaining IDs
		for (i, &id) in allocated_ids.iter().enumerate() {
			if i % 2 == 1 {
				id.remove();
			}
		}

		for id in new_ids {
			id.remove();
		}
	}

	#[test]
	fn test_connection_id_concurrent_like_access() {
		// Test that multiple rapid allocations and deallocations work correctly
		// This simulates concurrent-like access patterns
		let mut all_ids = Vec::new();

		// Rapid allocation burst
		for _ in 0..20 {
			all_ids.push(ConnectionId::next());
		}

		// Interleaved removal and allocation
		for i in 0..10 {
			if i < all_ids.len() {
				all_ids[i].remove();
			}
			all_ids.push(ConnectionId::next());
		}

		// Verify remaining IDs are unique
		let remaining_ids: Vec<_> = all_ids[10..].to_vec();
		for i in 0..remaining_ids.len() {
			for j in (i + 1)..remaining_ids.len() {
				assert_ne!(remaining_ids[i], remaining_ids[j]);
			}
		}

		// Clean up
		for id in remaining_ids {
			id.remove();
		}
	}

	#[test]
	fn test_connection_id_slab_reuse_behavior() {
		// Test that the slab correctly reuses slots after removal
		let id1 = ConnectionId::next();
		let id2 = ConnectionId::next();
		let id3 = ConnectionId::next();

		// Remove the middle ID
		id2.remove();

		// Allocate a new ID - it should reuse the slot
		let id4 = ConnectionId::next();

		// The new ID should be different from remaining allocated IDs
		assert_ne!(id1, id4);
		assert_ne!(id3, id4);

		// Clean up
		id1.remove();
		id3.remove();
		id4.remove();
	}

	#[test]
	fn test_connection_id_empty_and_refill() {
		// Test allocating, clearing all, then allocating again
		let mut first_batch = Vec::new();

		// First batch of allocations
		for _ in 0..10 {
			first_batch.push(ConnectionId::next());
		}

		// Remove all
		for id in first_batch {
			id.remove();
		}

		// Second batch of allocations
		let mut second_batch = Vec::new();
		for _ in 0..10 {
			second_batch.push(ConnectionId::next());
		}

		// All should be unique
		for i in 0..second_batch.len() {
			for j in (i + 1)..second_batch.len() {
				assert_ne!(second_batch[i], second_batch[j]);
			}
		}

		// Clean up
		for id in second_batch {
			id.remove();
		}
	}
}
