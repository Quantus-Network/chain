
// Option D

// use dilithium_crypto::ml_dsa_87;

// let (pk, sk) = ml_dsa_87::keypair();
// let pk_bytes: [u8; PUB_KEY_BYTES] = pk.to_bytes();
// let msg = runtime_call.encode();
// let sig_bytes = ml_dsa_87::sign(&msg, &sk, None);
// let sig = RezSignature::from_slice(&sig_bytes).unwrap();
// let account_id = sp_io::hashing::blake2_256(&pk_bytes).into();
// let extrinsic = UncheckedExtrinsic::new_signed(
//     call,
//     Address::Id(account_id),
//     RezMultiSignature::Rez(sig, pk_bytes),
//     signed_extra,
// );

// Signer for Option A-1

// let (pk, sk) = ml_dsa_87::keypair();
// let pk_bytes = pk.to_bytes().to_vec();
// let msg = runtime_call.encode();
// let sig_bytes = ml_dsa_87::sign(&msg, &sk, None);
// let mut combined = Vec::new();
// combined.extend_from_slice(&pk_bytes);
// combined.extend_from_slice(&sig_bytes);
// let account_id = sp_io::hashing::blake2_256(&pk_bytes).into();
// let extrinsic = UncheckedExtrinsic::new_signed(
//     call,
//     Address::Id(account_id),
//     RezMultiSignature::Rez(combined),
//     signed_extra,
// );

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn it_works() {
//         let result = add(2, 2);
//         assert_eq!(result, 4);
//     }
// }
