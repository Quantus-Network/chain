#![cfg_attr(not(feature = "std"), no_std)]

//! # DAG Consensus Pallet
//!
//! A pallet that implements GHOSTDAG consensus using Kaspa's battle-tested algorithms.
//! This pallet stores DAG block relations and manages GHOSTDAG data for each block.

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

mod ghostdag;
mod types;
pub mod weights;

pub use types::*;
pub use weights::WeightInfo;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{dispatch::DispatchResult, pallet_prelude::*, traits::Get};
	use frame_system::pallet_prelude::*;
	use sp_std::collections::btree_map::BTreeMap;

	/// GHOSTDAG K parameter - maximum anticone size for blue blocks
	pub const GHOSTDAG_K: u32 = 18;

	/// Maximum number of parents a block can have
	pub const MAX_BLOCK_PARENTS: u32 = 10;

	/// Default mergeset size limit
	pub const MERGESET_SIZE_LIMIT: u32 = 100;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;

		/// Maximum number of parents a block can reference
		#[pallet::constant]
		type MaxBlockParents: Get<u32>;

		/// GHOSTDAG k parameter
		#[pallet::constant]
		type GhostdagK: Get<u32>;

		/// Mergeset size limit
		#[pallet::constant]
		type MergesetSizeLimit: Get<u32>;
	}

	/// Storage for block parent-child relations
	#[pallet::storage]
	#[pallet::getter(fn block_relations)]
	pub type BlockRelations<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::Hash,
		BoundedVec<T::Hash, T::MaxBlockParents>,
		ValueQuery,
	>;

	/// Storage for GHOSTDAG data of each block
	#[pallet::storage]
	#[pallet::getter(fn ghostdag_data)]
	pub type GhostdagData<T: Config> =
		StorageMap<_, Blake2_128Concat, T::Hash, DagBlockData<T::Hash>, OptionQuery>;

	/// Storage for block blue work values
	#[pallet::storage]
	#[pallet::getter(fn blue_work)]
	pub type BlueWork<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::Hash,
		u128, // Using u128 instead of U256 for Substrate compatibility
		ValueQuery,
	>;

	/// Current DAG tips (blocks with no children)
	#[pallet::storage]
	#[pallet::getter(fn dag_tips)]
	pub type DagTips<T: Config> =
		StorageValue<_, BoundedVec<T::Hash, T::MaxBlockParents>, ValueQuery>;

	/// Virtual state representing the current DAG head
	#[pallet::storage]
	#[pallet::getter(fn virtual_state)]
	pub type VirtualState<T: Config> = StorageValue<_, VirtualDagState<T::Hash>, ValueQuery>;

	/// Reachability data for efficient DAG queries
	#[pallet::storage]
	pub type ReachabilityData<T: Config> =
		StorageDoubleMap<_, Blake2_128Concat, T::Hash, Blake2_128Concat, T::Hash, bool, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A new block was added to the DAG
		BlockAddedToDAG { block_hash: T::Hash, parents: Vec<T::Hash>, blue_score: u64 },
		/// Virtual state was updated
		VirtualStateUpdated { selected_parent: T::Hash, blue_work: u128 },
		/// DAG tips were updated
		DagTipsUpdated { new_tips: Vec<T::Hash> },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Block already exists in DAG
		BlockAlreadyExists,
		/// Invalid parent reference
		InvalidParent,
		/// Too many parents specified
		TooManyParents,
		/// GHOSTDAG validation failed
		GhostdagValidationFailed,
		/// Block not found
		BlockNotFound,
		/// Invalid DAG structure
		InvalidDAGStructure,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Add a new block to the DAG with specified parents
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::add_block_to_dag())]
		pub fn add_block_to_dag(
			origin: OriginFor<T>,
			block_hash: T::Hash,
			parents: Vec<T::Hash>,
		) -> DispatchResult {
			ensure_signed(origin)?;

			// Validate inputs
			ensure!(!GhostdagData::<T>::contains_key(&block_hash), Error::<T>::BlockAlreadyExists);
			ensure!(
				parents.len() <= T::MaxBlockParents::get() as usize,
				Error::<T>::TooManyParents
			);

			// Validate all parents exist (except for genesis)
			if !parents.is_empty() {
				for parent in &parents {
					ensure!(GhostdagData::<T>::contains_key(parent), Error::<T>::InvalidParent);
				}
			}

			// Calculate GHOSTDAG data for this block
			let ghostdag_data = Self::calculate_ghostdag_data(&parents)?;

			// Store the data
			let bounded_parents: BoundedVec<T::Hash, T::MaxBlockParents> =
				parents.clone().try_into().map_err(|_| Error::<T>::TooManyParents)?;

			BlockRelations::<T>::insert(&block_hash, &bounded_parents);
			GhostdagData::<T>::insert(&block_hash, &ghostdag_data);
			BlueWork::<T>::insert(&block_hash, ghostdag_data.blue_work);

			// Update DAG tips
			Self::update_dag_tips(block_hash, &parents)?;

			// Update virtual state
			Self::update_virtual_state()?;

			// Emit event
			Self::deposit_event(Event::BlockAddedToDAG {
				block_hash,
				parents,
				blue_score: ghostdag_data.blue_score,
			});

			Ok(())
		}

		/// Manually trigger virtual state recalculation
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::recalculate_virtual_state())]
		pub fn recalculate_virtual_state(origin: OriginFor<T>) -> DispatchResult {
			ensure_signed(origin)?;
			Self::update_virtual_state()?;
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		/// Calculate GHOSTDAG data for a block given its parents
		pub fn calculate_ghostdag_data(
			parents: &[T::Hash],
		) -> Result<DagBlockData<T::Hash>, Error<T>> {
			if parents.is_empty() {
				// Genesis block
				return Ok(DagBlockData {
					blue_score: 0,
					blue_work: 0,
					selected_parent: T::Hash::default(),
					mergeset_blues: BoundedVec::new(),
					mergeset_reds: BoundedVec::new(),
					blues_anticone_sizes: BoundedBTreeMap::new(),
				});
			}

			// Find selected parent (highest blue work)
			let selected_parent = Self::find_selected_parent(parents)?;

			// Build mergeset (blocks not in past of selected parent)
			let mergeset = Self::build_mergeset(&selected_parent, parents)?;

			// Apply GHOSTDAG coloring algorithm
			let mut ghostdag_data = DagBlockData {
				blue_score: 0,
				blue_work: 0,
				selected_parent,
				mergeset_blues: BoundedVec::new(),
				mergeset_reds: BoundedVec::new(),
				blues_anticone_sizes: BoundedBTreeMap::new(),
			};

			// Process mergeset in topological order
			let ordered_mergeset = Self::topological_sort(&mergeset);

			for candidate in ordered_mergeset {
				if Self::can_be_blue(&ghostdag_data, &candidate)? {
					let _ = ghostdag_data.mergeset_blues.try_push(candidate);
					// Update anticone sizes for blues
					Self::update_blues_anticone_sizes(&mut ghostdag_data, &candidate)?;
				} else {
					let _ = ghostdag_data.mergeset_reds.try_push(candidate);
				}
			}

			// Calculate final metrics
			let selected_parent_data =
				Self::ghostdag_data(&selected_parent).ok_or(Error::<T>::BlockNotFound)?;

			// Blue score = selected parent's blue score + 1 (for selected parent) + blue mergeset
			// size
			ghostdag_data.blue_score =
				selected_parent_data.blue_score + 1 + ghostdag_data.mergeset_blues.len() as u64;
			// Blue work = selected parent's blue work + 1 (for selected parent) + blue mergeset
			// work
			ghostdag_data.blue_work =
				selected_parent_data.blue_work +
					1 + Self::calculate_added_blue_work(&ghostdag_data.mergeset_blues);

			Ok(ghostdag_data)
		}

		/// Find the parent with highest blue work
		fn find_selected_parent(parents: &[T::Hash]) -> Result<T::Hash, Error<T>> {
			parents
				.iter()
				.max_by_key(|&&parent| Self::blue_work(&parent))
				.copied()
				.ok_or(Error::<T>::InvalidParent)
		}

		/// Build mergeset: blocks not in the past of selected parent
		fn build_mergeset(
			selected_parent: &T::Hash,
			parents: &[T::Hash],
		) -> Result<Vec<T::Hash>, Error<T>> {
			let mut mergeset = Vec::new();
			let mut visited = BTreeMap::new();

			// Start with non-selected parents
			let mut queue: Vec<T::Hash> =
				parents.iter().filter(|&&p| p != *selected_parent).copied().collect();

			while let Some(current) = queue.pop() {
				if visited.contains_key(&current) {
					continue;
				}

				visited.insert(current, true);

				// If not in past of selected parent, add to mergeset
				if !Self::is_dag_ancestor_of(*selected_parent, current) {
					mergeset.push(current);

					// Add parents to queue
					let current_parents = Self::block_relations(&current);
					for parent in current_parents.iter() {
						if !visited.contains_key(parent) {
							queue.push(*parent);
						}
					}
				}
			}

			Ok(mergeset)
		}

		/// Check if a candidate block can be colored blue (k-cluster validation)
		fn can_be_blue(
			ghostdag_data: &DagBlockData<T::Hash>,
			candidate: &T::Hash,
		) -> Result<bool, Error<T>> {
			let k = T::GhostdagK::get();

			// Check if we already have k+1 blues (including selected parent)
			if ghostdag_data.mergeset_blues.len() as u32 >= k {
				return Ok(false);
			}

			let mut candidate_anticone_size = 0u32;

			// Check against all existing blues
			for blue_block in &ghostdag_data.mergeset_blues {
				// If blocks are not ancestors of each other, they're in anticone
				if !Self::is_dag_ancestor_of(*blue_block, *candidate) &&
					!Self::is_dag_ancestor_of(*candidate, *blue_block)
				{
					candidate_anticone_size += 1;

					// Check if candidate's anticone would exceed k
					if candidate_anticone_size > k {
						return Ok(false);
					}

					// Check if this blue block's anticone would exceed k
					let blue_anticone_size =
						ghostdag_data.blues_anticone_sizes.get(blue_block).unwrap_or(&0);

					if *blue_anticone_size >= k {
						return Ok(false);
					}
				}
			}

			Ok(true)
		}

		/// Update anticone sizes for blues when adding a new blue block
		fn update_blues_anticone_sizes(
			ghostdag_data: &mut DagBlockData<T::Hash>,
			new_blue: &T::Hash,
		) -> Result<(), Error<T>> {
			let mut new_blue_anticone_size = 0u32;

			for blue_block in &ghostdag_data.mergeset_blues {
				if !Self::is_dag_ancestor_of(*blue_block, *new_blue) &&
					!Self::is_dag_ancestor_of(*new_blue, *blue_block)
				{
					// Increment anticone size for existing blue
					let current_size =
						ghostdag_data.blues_anticone_sizes.get(blue_block).unwrap_or(&0);
					let _ = ghostdag_data
						.blues_anticone_sizes
						.try_insert(*blue_block, current_size + 1);

					new_blue_anticone_size += 1;
				}
			}

			let _ =
				ghostdag_data.blues_anticone_sizes.try_insert(*new_blue, new_blue_anticone_size);
			Ok(())
		}

		/// Simple topological sort for mergeset ordering
		fn topological_sort(blocks: &[T::Hash]) -> Vec<T::Hash> {
			// For now, use blue work ordering
			// TODO: Implement proper topological sort based on DAG structure
			let mut sorted = blocks.to_vec();
			sorted.sort_by_key(|&block| Self::blue_work(&block));
			sorted
		}

		/// Calculate additional blue work from a list of blue blocks
		fn calculate_added_blue_work(blue_blocks: &[T::Hash]) -> u128 {
			// Simplified: each block contributes 1 unit of work
			// TODO: Use actual difficulty/work calculation
			blue_blocks.len() as u128
		}

		/// Check if ancestor is a DAG ancestor of descendant
		pub fn is_dag_ancestor_of(ancestor: T::Hash, descendant: T::Hash) -> bool {
			if ancestor == descendant {
				return true;
			}

			// Check stored reachability data first
			if ReachabilityData::<T>::contains_key(&ancestor, &descendant) {
				return ReachabilityData::<T>::get(&ancestor, &descendant);
			}

			// Compute reachability via BFS
			let result = Self::compute_reachability(ancestor, descendant);

			// Cache the result
			ReachabilityData::<T>::insert(&ancestor, &descendant, result);

			result
		}

		/// Compute reachability between two blocks via BFS
		fn compute_reachability(ancestor: T::Hash, descendant: T::Hash) -> bool {
			if ancestor == descendant {
				return true;
			}

			let mut visited = BTreeMap::new();
			let mut queue = Vec::new();
			queue.push(descendant);

			while let Some(current) = queue.pop() {
				if current == ancestor {
					return true;
				}

				if visited.contains_key(&current) {
					continue;
				}
				visited.insert(current, true);

				// Add parents to queue
				let parents = Self::block_relations(&current);
				for parent in parents.iter() {
					if !visited.contains_key(parent) {
						queue.push(*parent);
					}
				}
			}

			false
		}

		/// Update DAG tips when a new block is added
		fn update_dag_tips(new_block: T::Hash, parents: &[T::Hash]) -> Result<(), Error<T>> {
			let mut current_tips = Self::dag_tips();

			// Remove parents from tips (they're no longer tips)
			current_tips.retain(|tip| !parents.contains(tip));

			// Add new block as tip
			current_tips.try_push(new_block).map_err(|_| Error::<T>::TooManyParents)?;

			DagTips::<T>::put(&current_tips);

			Self::deposit_event(Event::DagTipsUpdated { new_tips: current_tips.to_vec() });

			Ok(())
		}

		/// Update virtual state to reflect current DAG head
		fn update_virtual_state() -> Result<(), Error<T>> {
			let tips = Self::dag_tips();

			if tips.is_empty() {
				return Ok(());
			}

			// Find the tip with highest blue work as selected parent
			let selected_parent = tips
				.iter()
				.max_by_key(|&&tip| Self::blue_work(&tip))
				.copied()
				.unwrap_or_default();

			let blue_work = Self::blue_work(&selected_parent);

			let virtual_state = VirtualDagState {
				parents: tips.iter().cloned().collect::<Vec<_>>().try_into().unwrap_or_default(),
				selected_parent,
				blue_work,
			};

			VirtualState::<T>::put(&virtual_state);

			Self::deposit_event(Event::VirtualStateUpdated { selected_parent, blue_work });

			Ok(())
		}

		/// Get the current virtual selected parent
		pub fn get_virtual_selected_parent() -> T::Hash {
			Self::virtual_state().selected_parent
		}

		/// Get all current DAG tips
		pub fn get_current_tips() -> Vec<T::Hash> {
			Self::dag_tips().to_vec()
		}

		/// Check if a block is in the blue set of another block
		pub fn is_blue_block(block: T::Hash, context: T::Hash) -> bool {
			if let Some(context_data) = Self::ghostdag_data(&context) {
				context_data.mergeset_blues.contains(&block) ||
					context_data.selected_parent == block
			} else {
				false
			}
		}
	}
}
