use primitive_types::{H256, U512};
use quantus_miner_api::{ApiResponseStatus, MiningRequest, MiningResponse, MiningResult};
/// Functions to interact with the external miner service
use reqwest::Client;

// Make functions pub(crate) or pub as needed
pub(crate) async fn submit_mining_job(
	client: &Client,
	miner_url: &str,
	job_id: &str,
	mining_hash: &H256,
	distance_threshold: U512,
	nonce_start: U512,
	nonce_end: U512,
) -> Result<(), String> {
	let request = MiningRequest {
		job_id: job_id.to_string(),
		mining_hash: hex::encode(mining_hash.as_bytes()),
		distance_threshold: distance_threshold.to_string(),
		nonce_start: format!("{:0128x}", nonce_start),
		nonce_end: format!("{:0128x}", nonce_end),
	};

	let response = client
		.post(format!("{}/mine", miner_url))
		.json(&request)
		.send()
		.await
		.map_err(|e| format!("Failed to send mining request: {}", e))?;

	let result: MiningResponse = response
		.json()
		.await
		.map_err(|e| format!("Failed to parse mining response: {}", e))?;

	if result.status != ApiResponseStatus::Accepted {
		return Err(format!("Mining job was not accepted: {:?}", result.status));
	}

	Ok(())
}

pub(crate) async fn check_mining_result(
	client: &Client,
	miner_url: &str,
	job_id: &str,
) -> Result<Option<[u8; 64]>, String> {
	let response = client
		.get(format!("{}/result/{}", miner_url, job_id))
		.send()
		.await
		.map_err(|e| format!("Failed to check mining result: {}", e))?;

	let result: MiningResult = response
		.json()
		.await
		.map_err(|e| format!("Failed to parse mining result: {}", e))?;

	match result.status {
		ApiResponseStatus::Completed =>
			if let Some(work_hex) = result.work {
				let nonce_bytes = hex::decode(&work_hex)
					.map_err(|e| format!("Failed to decode work hex '{}': {}", work_hex, e))?;
				if nonce_bytes.len() == 64 {
					let mut nonce = [0u8; 64];
					nonce.copy_from_slice(&nonce_bytes);
					Ok(Some(nonce))
				} else {
					Err(format!(
						"Invalid decoded work length: {} bytes (expected 64)",
						nonce_bytes.len()
					))
				}
			} else {
				Err("Missing 'work' field in completed mining result".to_string())
			},
		ApiResponseStatus::Running => Ok(None),
		ApiResponseStatus::NotFound => Err("Mining job not found".to_string()),
		ApiResponseStatus::Failed => Err("Mining job failed (miner reported)".to_string()),
		ApiResponseStatus::Cancelled =>
			Err("Mining job was cancelled (miner reported)".to_string()),
		ApiResponseStatus::Error => Err("Miner reported an unspecified error".to_string()),
		ApiResponseStatus::Accepted =>
			Err("Unexpected 'Accepted' status received from result endpoint".to_string()),
	}
}

pub(crate) async fn cancel_mining_job(
	client: &Client,
	miner_url: &str,
	job_id: &str,
) -> Result<(), String> {
	let response = client
		.post(format!("{}/cancel/{}", miner_url, job_id))
		.send()
		.await
		.map_err(|e| format!("Failed to cancel mining job: {}", e))?;

	let result: MiningResponse = response
		.json()
		.await
		.map_err(|e| format!("Failed to parse cancel response: {}", e))?;

	if result.status == ApiResponseStatus::Cancelled || result.status == ApiResponseStatus::NotFound
	{
		Ok(())
	} else {
		Err(format!("Failed to cancel mining job (unexpected status): {:?}", result.status))
	}
}
