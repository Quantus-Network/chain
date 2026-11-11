extern crate lru;

use lru::LruCache;
use std::num::NonZeroUsize;

#[test]
fn iter_mut_preserves_key_pointers() {
	for reverse in [false, true] {
		let mut cache = LruCache::new(NonZeroUsize::new(3).unwrap());
		for key in 1..=3 {
			cache.put(key, key * 10);
		}

		let mut iter = cache.iter_mut();
		while let Some((_, value)) = if reverse { iter.next_back() } else { iter.next() } {
			*value *= 2;
		}

		for key in 1..=3 {
			assert_eq!(cache.get(&key), Some(&(key * 20)));
		}
	}
}
