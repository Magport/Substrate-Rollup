#![cfg_attr(not(feature = "std"), no_std)]
use codec::{Decode, Encode};
use scale_info::TypeInfo;
use sp_inherents::{Error, InherentData, InherentIdentifier};

#[derive(Encode, Decode, sp_core::RuntimeDebug, Clone, PartialEq, TypeInfo)]
pub struct AvailInherentDataProvider {
	pub last_submit_block_confirm: u32,
}
pub const INHERENT_IDENTIFIER: InherentIdentifier = *b"availiht";

#[cfg(feature = "std")]
impl AvailInherentDataProvider {
	pub fn new(block_number: u32) -> AvailInherentDataProvider {
		AvailInherentDataProvider { last_submit_block_confirm: block_number }
	}
}

#[derive(Clone)]
pub struct AvailRecord {
	pub last_submit_block: u32,
	pub last_submit_block_confirm: u32,
	pub last_avail_scan_block: u32,
	pub last_avail_scan_block_confirm: u32,
}

#[cfg(feature = "std")]
#[async_trait::async_trait]
impl sp_inherents::InherentDataProvider for AvailInherentDataProvider {
	/// Provide the inherent data into the given `inherent_data`.
	async fn provide_inherent_data(&self, inherent_data: &mut InherentData) -> Result<(), Error> {
		inherent_data.put_data(INHERENT_IDENTIFIER, &self)
	}
	async fn try_handle_error(
		&self,
		_: &sp_inherents::InherentIdentifier,
		_: &[u8],
	) -> Option<Result<(), sp_inherents::Error>> {
		None
	}
}

sp_api::decl_runtime_apis! {
	#[api_version(2)]
	pub trait AvailRuntimeApi
	{
		fn last_submit_block_confirm()-> u32;
		fn last_avail_scan_block()->u32;
	}
}
