//! GHOSTDAG helper functions
//!
//! This module contains simplified versions of GHOSTDAG algorithms
//! adapted for Substrate pallet usage.

use crate::types::DagBlockData;

/// Helper functions for working with DagBlockData
impl<Hash: Clone + Ord + Default> DagBlockData<Hash> {
	/// Create new DAG block data with selected parent
	pub fn new_with_selected_parent(selected_parent: Hash) -> Self {
		Self { selected_parent, ..Default::default() }
	}

	/// Add a block to the blue set (handles capacity limits)
	pub fn try_add_blue(&mut self, block: Hash, anticone_size: u32) -> Result<(), &'static str> {
		self.mergeset_blues.try_push(block.clone()).map_err(|_| "Blue mergeset full")?;
		self.blues_anticone_sizes
			.try_insert(block, anticone_size)
			.map_err(|_| "Anticone map full")?;
		Ok(())
	}

	/// Add a block to the red set (handles capacity limits)
	pub fn try_add_red(&mut self, block: Hash) -> Result<(), &'static str> {
		self.mergeset_reds.try_push(block).map_err(|_| "Red mergeset full")
	}

	/// Finalize the block data with calculated metrics
	pub fn finalize(&mut self, blue_score: u64, blue_work: u128) {
		self.blue_score = blue_score;
		self.blue_work = blue_work;
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_dag_block_data_creation() {
		let block_data = DagBlockData::<u32>::default();
		assert_eq!(block_data.blue_score, 0);
		assert_eq!(block_data.blue_work, 0);
		assert_eq!(block_data.mergeset_blues.len(), 0);
		assert_eq!(block_data.mergeset_reds.len(), 0);
	}

	#[test]
	fn test_dag_block_data_helper_methods() {
		let block_data = DagBlockData::<u32>::new_with_selected_parent(42);
		assert_eq!(block_data.selected_parent, 42);
		assert_eq!(block_data.blue_score, 0);
		assert_eq!(block_data.blue_work, 0);
		assert_eq!(block_data.mergeset_blues.len(), 0);
		assert_eq!(block_data.mergeset_reds.len(), 0);
	}
}
