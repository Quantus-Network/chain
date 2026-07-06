// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use syn::parse_quote;

#[test]
fn test_weight_with_trailing_tokens_is_rejected() {
	assert_pallet_parse_error! {
		#[manifest_dir("../examples/basic")]
		#[error_regex("unexpected trailing tokens in pallet attribute")]
		#[frame_support::pallet]
		pub mod pallet {
			#[pallet::config]
			pub trait Config: frame_system::Config {}

			#[pallet::pallet]
			pub struct Pallet<T>(_);

			#[pallet::call]
			impl<T: Config> Pallet<T> {
				#[pallet::call_index(0)]
				#[pallet::weight(Weight::zero(), DispatchClass::Operational)]
				pub fn noop(origin: OriginFor<T>) -> DispatchResult {
					Ok(())
				}
			}
		}
	}
}

#[test]
fn test_call_index_with_trailing_tokens_is_rejected() {
	assert_pallet_parse_error! {
		#[manifest_dir("../examples/basic")]
		#[error_regex("unexpected trailing tokens in pallet attribute")]
		#[frame_support::pallet]
		pub mod pallet {
			#[pallet::config]
			pub trait Config: frame_system::Config {}

			#[pallet::pallet]
			pub struct Pallet<T>(_);

			#[pallet::call]
			impl<T: Config> Pallet<T> {
				#[pallet::call_index(0, 1)]
				#[pallet::weight(Weight::zero())]
				pub fn noop(origin: OriginFor<T>) -> DispatchResult {
					Ok(())
				}
			}
		}
	}
}

#[test]
fn test_well_formed_call_attributes_parse() {
	assert_pallet_parses! {
		#[manifest_dir("../examples/basic")]
		#[frame_support::pallet]
		pub mod pallet {
			#[pallet::config]
			pub trait Config: frame_system::Config {}

			#[pallet::pallet]
			pub struct Pallet<T>(_);

			#[pallet::call]
			impl<T: Config> Pallet<T> {
				#[pallet::call_index(0)]
				#[pallet::weight(Weight::zero())]
				pub fn noop(origin: OriginFor<T>) -> DispatchResult {
					Ok(())
				}
			}
		}
	};
}
