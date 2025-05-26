//! Custom origins for governance interventions.

#[frame_support::pallet]
pub mod pallet_custom_origins {
    use crate::{Balance, UNIT, MICRO_UNIT};
    use frame_support::pallet_prelude::*;

    #[pallet::config]
    pub trait Config: frame_system::Config {}

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[derive(PartialEq, Eq, Clone, MaxEncodedLen, Encode, Decode, TypeInfo, RuntimeDebug)]
    #[pallet::origin]
    pub enum Origin {
        /// Origin for cancelling slashes.
        Treasurer,
        /// Origin able to spend up to 1 KSM from the treasury at once.
        SmallTipper,
        /// Origin able to spend up to 5 KSM from the treasury at once.
        BigTipper,
        /// Origin able to spend up to 50 KSM from the treasury at once.
        SmallSpender,
        /// Origin able to spend up to 500 KSM from the treasury at once.
        MediumSpender,
        /// Origin able to spend up to 5,000 KSM from the treasury at once.
        BigSpender,
    }

    macro_rules! decl_unit_ensures {
        ( $name:ident: $success_type:ty = $success:expr ) => {
            pub struct $name;
            impl<O: Into<Result<Origin, O>> + From<Origin>>
                EnsureOrigin<O> for $name
            {
                type Success = $success_type;
                fn try_origin(o: O) -> Result<Self::Success, O> {
                    o.into().and_then(|o| match o {
                        Origin::$name => Ok($success),
                        r => Err(O::from(r)),
                    })
                }
                #[cfg(feature = "runtime-benchmarks")]
                fn try_successful_origin() -> Result<O, ()> {
                    Ok(O::from(Origin::$name))
                }
            }
        };
        ( $name:ident ) => { decl_unit_ensures! { $name : () = () } };
        ( $name:ident: $success_type:ty = $success:expr, $( $rest:tt )* ) => {
            decl_unit_ensures! { $name: $success_type = $success }
            decl_unit_ensures! { $( $rest )* }
        };
        ( $name:ident, $( $rest:tt )* ) => {
            decl_unit_ensures! { $name }
            decl_unit_ensures! { $( $rest )* }
        };
        () => {}
    }
    decl_unit_ensures!(
        Treasurer,
    );

    macro_rules! decl_ensure {
		(
			$vis:vis type $name:ident: EnsureOrigin<Success = $success_type:ty> {
				$( $item:ident = $success:expr, )*
			}
		) => {
			$vis struct $name;
			impl<O: Into<Result<Origin, O>> + From<Origin>>
				EnsureOrigin<O> for $name
			{
				type Success = $success_type;
				fn try_origin(o: O) -> Result<Self::Success, O> {
					o.into().and_then(|o| match o {
						$(
							Origin::$item => Ok($success),
						)*
						//r => Err(O::from(r)),
					})
				}
				#[cfg(feature = "runtime-benchmarks")]
				fn try_successful_origin() -> Result<O, ()> {
					// By convention the more privileged origins go later, so for greatest chance
					// of success, we want the last one.
					let _result: Result<O, ()> = Err(());
					$(
						let _result: Result<O, ()> = Ok(O::from(Origin::$item));
					)*
					_result
				}
			}
		}
	}

    decl_ensure! {
        pub type Spender: EnsureOrigin<Success = Balance> {
            SmallTipper = 250 * 3 * MICRO_UNIT,
            BigTipper = 10 * UNIT,
            SmallSpender = 100 * UNIT,
            MediumSpender = 1_000 * UNIT,
            BigSpender = 10_000 * UNIT,
            Treasurer = 100_000 * UNIT,
        }
    }
}

// Re-export the pallet and its types
pub use pallet_custom_origins::{Origin, Spender, Treasurer};