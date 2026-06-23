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

#[cfg(feature = "decode")]
use codec::Decode;
#[cfg(feature = "serde_full")]
use serde::Serialize;

use super::{RuntimeMetadataPrefixed, META_RESERVED};
use codec::{Compact, Encode};
use scale_info::{
	form::{Form, MetaForm, PortableForm},
	prelude::{collections::BTreeMap, vec::Vec},
	IntoPortable, PortableRegistry, Registry,
};

// These types have not changed, so we re-export from our v14/v15 definitions:
pub use super::v14::{StorageEntryModifier, StorageEntryType, StorageHasher};
pub use super::v15::{CustomMetadata, CustomValueMetadata, OuterEnums};

/// The metadata for a method or function parameter. This is identical to
/// [`crate::v15::RuntimeApiMethodParamMetadata`].
pub type FunctionParamMetadata<T> = super::v15::RuntimeApiMethodParamMetadata<T>;

/// Latest runtime metadata.
pub type RuntimeMetadataLastVersion = RuntimeMetadataV16;

impl From<RuntimeMetadataLastVersion> for super::RuntimeMetadataPrefixed {
	fn from(metadata: RuntimeMetadataLastVersion) -> RuntimeMetadataPrefixed {
		RuntimeMetadataPrefixed(META_RESERVED, super::RuntimeMetadata::V16(metadata))
	}
}

/// The metadata of a runtime.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
pub struct RuntimeMetadataV16 {
	/// Type registry containing all types used in the metadata.
	pub types: PortableRegistry,
	/// Metadata of all the pallets.
	pub pallets: Vec<PalletMetadata<PortableForm>>,
	/// Metadata of the extrinsic.
	pub extrinsic: ExtrinsicMetadata<PortableForm>,
	/// Metadata of the Runtime API.
	pub apis: Vec<RuntimeApiMetadata<PortableForm>>,
	/// The outer enums types as found in the runtime.
	pub outer_enums: OuterEnums<PortableForm>,
	/// Allows users to add custom types to the metadata.
	pub custom: CustomMetadata<PortableForm>,
}

impl RuntimeMetadataV16 {
	/// Create a new instance of [`RuntimeMetadataV16`].
	pub fn new(
		pallets: Vec<PalletMetadata>,
		extrinsic: ExtrinsicMetadata,
		apis: Vec<RuntimeApiMetadata>,
		outer_enums: OuterEnums,
		custom: CustomMetadata,
	) -> Self {
		let mut registry = Registry::new();
		// extrinsic types need to be collected first to ensure CheckMetadataHash hash
		// is stable across different metadata versions
		let extrinsic = extrinsic.into_portable(&mut registry);
		let pallets = registry.map_into_portable(pallets);
		let apis = registry.map_into_portable(apis);
		let outer_enums = outer_enums.into_portable(&mut registry);
		let custom = custom.into_portable(&mut registry);

		Self {
			types: registry.into(),
			pallets,
			extrinsic,
			apis,
			outer_enums,
			custom,
		}
	}
}

/// Metadata of a runtime trait.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct RuntimeApiMetadata<T: Form = MetaForm> {
	/// Trait name.
	pub name: T::String,
	/// Trait methods.
	pub methods: Vec<RuntimeApiMethodMetadata<T>>,
	/// Trait documentation.
	pub docs: Vec<T::String>,
	/// Runtime API version.
	pub version: Compact<u32>,
	/// Deprecation info.
	pub deprecation_info: ItemDeprecationInfo<T>,
}

impl IntoPortable for RuntimeApiMetadata {
	type Output = RuntimeApiMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		RuntimeApiMetadata {
			name: self.name.into_portable(registry),
			methods: registry.map_into_portable(self.methods),
			docs: registry.map_into_portable(self.docs),
			version: self.version,
			deprecation_info: self.deprecation_info.into_portable(registry),
		}
	}
}

/// Metadata of a runtime method.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct RuntimeApiMethodMetadata<T: Form = MetaForm> {
	/// Method name.
	pub name: T::String,
	/// Method parameters.
	pub inputs: Vec<FunctionParamMetadata<T>>,
	/// Method output.
	pub output: T::Type,
	/// Method documentation.
	pub docs: Vec<T::String>,
	/// Deprecation info
	pub deprecation_info: ItemDeprecationInfo<T>,
}

impl IntoPortable for RuntimeApiMethodMetadata {
	type Output = RuntimeApiMethodMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		RuntimeApiMethodMetadata {
			name: self.name.into_portable(registry),
			inputs: registry.map_into_portable(self.inputs),
			output: registry.register_type(&self.output),
			docs: registry.map_into_portable(self.docs),
			deprecation_info: self.deprecation_info.into_portable(registry),
		}
	}
}

/// Metadata of the extrinsic used by the runtime.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct ExtrinsicMetadata<T: Form = MetaForm> {
	/// Extrinsic versions supported by the runtime.
	pub versions: Vec<u8>,
	/// The type of the address that signs the extrinsic
	pub address_ty: T::Type,
	/// The type of the outermost Call enum.
	// Dev note: this is also exposed in outer_enums, but we duplicate
	// it here so that ExtrinsicMetadata, on its own, provides everything
	// needed to decode an extrinsic.
	pub call_ty: T::Type,
	/// The type of the extrinsic's signature.
	pub signature_ty: T::Type,
	/// A mapping of supported transaction extrinsic versions to their respective transaction extension indexes.
	///
	/// For each supported version number, list the indexes, in order, of the extensions used.
	pub transaction_extensions_by_version: BTreeMap<u8, Vec<Compact<u32>>>,
	/// The transaction extensions in the order they appear in the extrinsic.
	pub transaction_extensions: Vec<TransactionExtensionMetadata<T>>,
}

impl IntoPortable for ExtrinsicMetadata {
	type Output = ExtrinsicMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		// the collection order needs to be stable across different metadata versions
		// to ensure CheckMetadataHash hash is invariant
		ExtrinsicMetadata {
			versions: self.versions,
			address_ty: registry.register_type(&self.address_ty),
			call_ty: registry.register_type(&self.call_ty),
			signature_ty: registry.register_type(&self.signature_ty),
			transaction_extensions_by_version: self.transaction_extensions_by_version,
			transaction_extensions: registry.map_into_portable(self.transaction_extensions),
		}
	}
}

/// Metadata of an extrinsic's transaction extension.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct TransactionExtensionMetadata<T: Form = MetaForm> {
	/// The unique transaction extension identifier, which may be different from the type name.
	pub identifier: T::String,
	/// The type of the transaction extension, with the data to be included in the extrinsic.
	pub ty: T::Type,
	/// The type of the implicit data, with the data to be included in the signed payload.
	pub implicit: T::Type,
}

impl IntoPortable for TransactionExtensionMetadata {
	type Output = TransactionExtensionMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		TransactionExtensionMetadata {
			identifier: self.identifier.into_portable(registry),
			ty: registry.register_type(&self.ty),
			implicit: registry.register_type(&self.implicit),
		}
	}
}

/// All metadata about an runtime pallet.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct PalletMetadata<T: Form = MetaForm> {
	/// Pallet name.
	pub name: T::String,
	/// Pallet storage metadata.
	pub storage: Option<PalletStorageMetadata<T>>,
	/// Pallet calls metadata.
	pub calls: Option<PalletCallMetadata<T>>,
	/// Pallet event metadata.
	pub event: Option<PalletEventMetadata<T>>,
	/// Pallet constants metadata.
	pub constants: Vec<PalletConstantMetadata<T>>,
	/// Pallet error metadata.
	pub error: Option<PalletErrorMetadata<T>>,
	/// Config's trait associated types.
	pub associated_types: Vec<PalletAssociatedTypeMetadata<T>>,
	/// Pallet view functions metadata.
	pub view_functions: Vec<PalletViewFunctionMetadata<T>>,
	/// Define the index of the pallet, this index will be used for the encoding of pallet event,
	/// call and origin variants.
	pub index: u8,
	/// Pallet documentation.
	pub docs: Vec<T::String>,
	/// Deprecation info
	pub deprecation_info: ItemDeprecationInfo<T>,
}

impl IntoPortable for PalletMetadata {
	type Output = PalletMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		PalletMetadata {
			name: self.name.into_portable(registry),
			storage: self.storage.map(|storage| storage.into_portable(registry)),
			calls: self.calls.map(|calls| calls.into_portable(registry)),
			event: self.event.map(|event| event.into_portable(registry)),
			constants: registry.map_into_portable(self.constants),
			error: self.error.map(|error| error.into_portable(registry)),
			associated_types: registry.map_into_portable(self.associated_types),
			view_functions: registry.map_into_portable(self.view_functions),
			index: self.index,
			docs: registry.map_into_portable(self.docs),
			deprecation_info: self.deprecation_info.into_portable(registry),
		}
	}
}

/// Metadata for all calls in a pallet.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct PalletCallMetadata<T: Form = MetaForm> {
	/// The corresponding enum type for the pallet call.
	pub ty: T::Type,
	/// Deprecation status of the pallet call
	pub deprecation_info: EnumDeprecationInfo<T>,
}

impl IntoPortable for PalletCallMetadata {
	type Output = PalletCallMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		PalletCallMetadata {
			ty: registry.register_type(&self.ty),
			deprecation_info: self.deprecation_info.into_portable(registry),
		}
	}
}

/// All metadata of the pallet's storage.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct PalletStorageMetadata<T: Form = MetaForm> {
	/// The common prefix used by all storage entries.
	pub prefix: T::String,
	/// Metadata for all storage entries.
	pub entries: Vec<StorageEntryMetadata<T>>,
}

impl IntoPortable for PalletStorageMetadata {
	type Output = PalletStorageMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		PalletStorageMetadata {
			prefix: self.prefix.into_portable(registry),
			entries: registry.map_into_portable(self.entries),
		}
	}
}

/// Metadata about one storage entry.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct StorageEntryMetadata<T: Form = MetaForm> {
	/// Variable name of the storage entry.
	pub name: T::String,
	/// An `Option` modifier of that storage entry.
	pub modifier: StorageEntryModifier,
	/// Type of the value stored in the entry.
	pub ty: StorageEntryType<T>,
	/// Default value (SCALE encoded).
	pub default: Vec<u8>,
	/// Storage entry documentation.
	pub docs: Vec<T::String>,
	/// Deprecation info
	pub deprecation_info: ItemDeprecationInfo<T>,
}

impl IntoPortable for StorageEntryMetadata {
	type Output = StorageEntryMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		StorageEntryMetadata {
			name: self.name.into_portable(registry),
			modifier: self.modifier,
			ty: self.ty.into_portable(registry),
			default: self.default,
			docs: registry.map_into_portable(self.docs),
			deprecation_info: self.deprecation_info.into_portable(registry),
		}
	}
}

/// Metadata about the pallet Event type.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct PalletEventMetadata<T: Form = MetaForm> {
	/// The Event type.
	pub ty: T::Type,
	/// Deprecation info
	pub deprecation_info: EnumDeprecationInfo<T>,
}

impl IntoPortable for PalletEventMetadata {
	type Output = PalletEventMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		PalletEventMetadata {
			ty: registry.register_type(&self.ty),
			deprecation_info: self.deprecation_info.into_portable(registry),
		}
	}
}

/// Metadata about one pallet constant.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct PalletConstantMetadata<T: Form = MetaForm> {
	/// Name of the pallet constant.
	pub name: T::String,
	/// Type of the pallet constant.
	pub ty: T::Type,
	/// Value stored in the constant (SCALE encoded).
	pub value: Vec<u8>,
	/// Documentation of the constant.
	pub docs: Vec<T::String>,
	/// Deprecation info
	pub deprecation_info: ItemDeprecationInfo<T>,
}

impl IntoPortable for PalletConstantMetadata {
	type Output = PalletConstantMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		PalletConstantMetadata {
			name: self.name.into_portable(registry),
			ty: registry.register_type(&self.ty),
			value: self.value,
			docs: registry.map_into_portable(self.docs),
			deprecation_info: self.deprecation_info.into_portable(registry),
		}
	}
}

/// Metadata about a pallet error.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct PalletErrorMetadata<T: Form = MetaForm> {
	/// The error type information.
	pub ty: T::Type,
	/// Deprecation info
	pub deprecation_info: EnumDeprecationInfo<T>,
}

impl IntoPortable for PalletErrorMetadata {
	type Output = PalletErrorMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		PalletErrorMetadata {
			ty: registry.register_type(&self.ty),
			deprecation_info: self.deprecation_info.into_portable(registry),
		}
	}
}

/// Metadata of a pallet's associated type.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct PalletAssociatedTypeMetadata<T: Form = MetaForm> {
	/// The name of the associated type.
	pub name: T::String,
	/// The type of the associated type.
	pub ty: T::Type,
	/// The documentation of the associated type.
	pub docs: Vec<T::String>,
}

impl IntoPortable for PalletAssociatedTypeMetadata {
	type Output = PalletAssociatedTypeMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		PalletAssociatedTypeMetadata {
			name: self.name.into_portable(registry),
			ty: registry.register_type(&self.ty),
			docs: registry.map_into_portable(self.docs),
		}
	}
}

/// Metadata about a pallet view function.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct PalletViewFunctionMetadata<T: Form = MetaForm> {
	/// Method id.
	pub id: [u8; 32],
	/// Method name.
	pub name: T::String,
	/// Method parameters.
	pub inputs: Vec<FunctionParamMetadata<T>>,
	/// Method output.
	pub output: T::Type,
	/// Method documentation.
	pub docs: Vec<T::String>,
	/// Deprecation info
	pub deprecation_info: ItemDeprecationInfo<T>,
}

impl IntoPortable for PalletViewFunctionMetadata {
	type Output = PalletViewFunctionMetadata<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		PalletViewFunctionMetadata {
			id: self.id,
			name: self.name.into_portable(registry),
			inputs: registry.map_into_portable(self.inputs),
			output: registry.register_type(&self.output),
			docs: registry.map_into_portable(self.docs),
			deprecation_info: self.deprecation_info.into_portable(registry),
		}
	}
}

/// Deprecation information for generic items.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub enum ItemDeprecationInfo<T: Form = MetaForm> {
	/// Item is not deprecated.
	NotDeprecated,
	/// Item is fully deprecated without a note.
	DeprecatedWithoutNote,
	/// Item is fully deprecated with a note and an optional `since` field.
	Deprecated {
		/// Note explaining the deprecation
		note: T::String,
		/// Optional value for noting the version when the deprecation occurred.
		since: Option<T::String>,
	},
}

impl IntoPortable for ItemDeprecationInfo {
	type Output = ItemDeprecationInfo<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		match self {
			Self::NotDeprecated => ItemDeprecationInfo::NotDeprecated,
			Self::DeprecatedWithoutNote => ItemDeprecationInfo::DeprecatedWithoutNote,
			Self::Deprecated { note, since } => {
				let note = note.into_portable(registry);
				let since = since.map(|x| x.into_portable(registry));
				ItemDeprecationInfo::Deprecated { note, since }
			}
		}
	}
}

/// Deprecation information for enums in which specific variants can be deprecated.
/// If the map is empty, then nothing is deprecated.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub struct EnumDeprecationInfo<T: Form = MetaForm>(pub BTreeMap<u8, VariantDeprecationInfo<T>>);

impl<T: Form> EnumDeprecationInfo<T> {
	/// Construct an instance in which nothing is marked for deprecation.
	pub fn nothing_deprecated() -> Self {
		Self(BTreeMap::new())
	}

	/// Are any variants deprecated?
	pub fn has_deprecated_variants(&self) -> bool {
		!self.0.is_empty()
	}

	/// Is a specific variant deprecated?
	pub fn is_variant_deprecated(&self, variant_index: u8) -> bool {
		self.0.contains_key(&variant_index)
	}
}

impl IntoPortable for EnumDeprecationInfo {
	type Output = EnumDeprecationInfo<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		let entries = self
			.0
			.into_iter()
			.map(|(k, entry)| (k, entry.into_portable(registry)));
		EnumDeprecationInfo(entries.collect())
	}
}

/// Deprecation information for an item or variant in the metadata.
// Dev note: we use #[codec(index)] here to align the indexes with those
// of ItemDeprecationInfo, allowing both can decode into this asa convenience.
#[derive(Clone, PartialEq, Eq, Encode, Debug)]
#[cfg_attr(feature = "decode", derive(Decode))]
#[cfg_attr(feature = "serde_full", derive(Serialize))]
#[cfg_attr(
	feature = "serde_full",
	serde(bound(serialize = "T::Type: Serialize, T::String: Serialize"))
)]
pub enum VariantDeprecationInfo<T: Form = MetaForm> {
	/// Variant is deprecated without a note.
	#[codec(index = 1)]
	DeprecatedWithoutNote,
	/// Variant is deprecated with a note and an optional `since` field.
	#[codec(index = 2)]
	Deprecated {
		/// Note explaining the deprecation
		note: T::String,
		/// Optional value for noting the version when the deprecation occurred.
		since: Option<T::String>,
	},
}

impl IntoPortable for VariantDeprecationInfo {
	type Output = VariantDeprecationInfo<PortableForm>;

	fn into_portable(self, registry: &mut Registry) -> Self::Output {
		match self {
			Self::Deprecated { note, since } => {
				let note = note.into_portable(registry);
				let since = since.map(|x| x.into_portable(registry));
				VariantDeprecationInfo::Deprecated { note, since }
			}
			Self::DeprecatedWithoutNote => VariantDeprecationInfo::DeprecatedWithoutNote,
		}
	}
}
