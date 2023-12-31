#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
pub use weights::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		/// Type representing the weight of this pallet
		type WeightInfo: WeightInfo;
	}

	#[pallet::storage]
	#[pallet::getter(fn last_submit_block_confirm)]
	pub(super) type LastSubmitBlockConfirm<T: Config> = StorageValue<_, u32, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn last_submit_block)]
	pub(super) type LastSubmitBlock<T: Config> = StorageValue<_, u32, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn last_avail_scan_block)]
	pub(super) type LastAvailScanBlock<T: Config> = StorageValue<_, u32, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [something, who]
		SomethingStored { something: u32, who: T::AccountId },
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Error names should be descriptive.
		NoneValue,
		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
	}

	#[pallet::inherent]
	impl<T: Config> ProvideInherent for Pallet<T> {
		type Call = Call<T>;
		type Error = MakeFatalError<()>;

		const INHERENT_IDENTIFIER: InherentIdentifier = primitives_avail::INHERENT_IDENTIFIER;
		fn create_inherent(data: &InherentData) -> Option<Self::Call> {
			let data: primitives_avail::AvailInherentDataProvider = data
				.get_data(&primitives_avail::INHERENT_IDENTIFIER)
				.ok()
				.flatten()
				.expect("there is not data to be posted; qed");
			Some(Call::avail_data { data })
		}
		fn is_inherent(call: &Self::Call) -> bool {
			matches!(call, Call::avail_data { .. })
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::call_index(0)]
		#[pallet::weight((0, DispatchClass::Mandatory))]
		pub fn avail_data(
			origin: OriginFor<T>,
			data: primitives_avail::AvailInherentDataProvider,
		) -> DispatchResultWithPostInfo {
			ensure_none(origin)?;
			LastSubmitBlockConfirm::<T>::set(data.last_submit_block_confirm);
			LastSubmitBlock::<T>::set(data.last_submit_block);
			Ok(().into())
		}
	}
}
