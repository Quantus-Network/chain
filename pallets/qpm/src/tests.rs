use super::*;
use crate::{CompactPrediction, Pallet, PredictionList, Predictions};
use mock::*;

#[test]
fn block_time_lookup_works() {
	new_test_ext().execute_with(|| {
		set_mock_block_time(1, 1000);
		set_mock_block_time(2, 2000);

		assert_eq!(<MockBlockTimes<u64, u64> as BlockInfo<u64, u64>>::block_time(1), 1000);
		assert_eq!(<MockBlockTimes<u64, u64> as BlockInfo<u64, u64>>::block_time(2), 2000);
		assert_eq!(<MockBlockTimes<u64, u64> as BlockInfo<u64, u64>>::block_time(3), 0);
	});
}

#[test]
fn closest_prediction_logic_works() {
	new_test_ext().execute_with(|| {
		let block_number = 10u64;
		let preds: PredictionList<AccountId, u64, MaxPredictions> = vec![
			CompactPrediction { moment: 1000, account: 1 },
			CompactPrediction { moment: 2000, account: 2 },
			CompactPrediction { moment: 3000, account: 3 },
		]
		.try_into()
		.expect("msg");

		Predictions::<Test>::insert(block_number, preds);

		set_mock_block_time(block_number, 2100);

		let (winner, total) = Pallet::<Test>::resolve_predictions(block_number).unwrap();
		assert_eq!(total, 3);
		assert_eq!(winner.account, 2);
		assert_eq!(winner.moment, 2000);

		set_mock_block_time(block_number, 2900);
		let (winner, _) = Pallet::<Test>::resolve_predictions(block_number).unwrap();
		assert_eq!(winner.account, 3);
		assert_eq!(winner.moment, 3000);

		set_mock_block_time(block_number, 1000);
		let (winner, _) = Pallet::<Test>::resolve_predictions(block_number).unwrap();
		assert_eq!(winner.account, 1);
		assert_eq!(winner.moment, 1000);
	});
}
