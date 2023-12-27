use anyhow::Result as OtherResult;
use avail_subxt::{
	api as AvailApi,
	api::runtime_types::bounded_collections::bounded_vec::BoundedVec as OtherBoundedVec,
	primitives::AvailExtrinsicParams, AvailConfig,
};
use subxt::ext::sp_core::sr25519::Pair as OtherPair;
type AvailPairSigner = subxt::tx::PairSigner<AvailConfig, OtherPair>;
use avail_subxt::{
	api::runtime_types::{avail_core::AppId, da_control::pallet::Call as DaCall},
	avail::AppUncheckedExtrinsic,
	Call,
};
use codec::Decode;
use futures::lock::Mutex;
use futures_timer::Delay;
use primitives_avail::{AvailRecord, AvailRuntimeApi};
use sc_client_api::{backend::AuxStore, BlockBackend, BlockOf};
use sc_service::{error::Error as ServiceError, TaskManager};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_runtime::{
	generic,
	traits::{Block as BlockT, Header as HeaderT, NumberFor},
	SaturatedConversion,
};
use std::{error::Error, sync::Arc};
use subxt::{rpc::types::BlockNumber, OnlineClient};

fn signer_from_seed(seed: &str) -> OtherResult<AvailPairSigner> {
	let pair = <OtherPair as subxt::ext::sp_core::Pair>::from_string(seed, None)?;
	let signer = AvailPairSigner::new(pair);
	Ok(signer)
}

async fn query_block_exist<T, B>(
	avail_client: &OnlineClient<AvailConfig>,
	client: Arc<T>,
	block_num: u32,
	block_number_solo: u32,
) -> Result<bool, Box<dyn Error>>
where
	B: BlockT,
	T: ProvideRuntimeApi<B>
		+ BlockOf
		+ AuxStore
		+ HeaderBackend<B>
		+ Send
		+ Sync
		+ 'static
		+ BlockBackend<B>,
	<<B as BlockT>::Header as HeaderT>::Number: Into<u32>,
{
	let block_number = Some(BlockNumber::from(block_num));
	let block_hash = avail_client.rpc().block_hash(block_number).await?;
	// log::info!("block_hash:{:?}", block_hash);
	let rpc_response = avail_client.rpc().block(block_hash).await?;
	let submitted_block = rpc_response.unwrap();
	let find_result = submitted_block
		.block
		.extrinsics
		.into_iter()
		.filter_map(|chain_block_ext| {
			AppUncheckedExtrinsic::try_from(chain_block_ext).map(|ext| ext).ok()
		})
		.filter_map(|extrinsic| Some(extrinsic.function))
		.find(|call| match call {
			Call::DataAvailability(da_call) => match da_call {
				DaCall::submit_data { data } => {
					// log::info!("=======get:{:?}", data);
					let rollup_block = node_template_runtime::Block::decode(&mut &data.0[..]);
					// log::info!("=======get block:{:?}", rollup_block);
					let rollup_block_hash = client.block_hash(block_number_solo.into());
					let mut find_flag = false;
					if let Ok(rollup_hash) = rollup_block_hash {
						let rollup_block: Option<sp_runtime::generic::SignedBlock<B>> =
							BlockBackend::block(&*client, rollup_hash.unwrap()).unwrap();
						// log::info!("{:?}", rollup_block);
						if let Some(block) = rollup_block {
							let bytes = block.block.encode();
							if bytes == data.0[..] {
								find_flag = true;
							}
						}
					}
					find_flag
				},
				_ => false,
			},
			_ => false,
		});
	match find_result {
		Some(_) => Ok(true),
		None => Ok(false),
	}
}
async fn get_avail_latest_height(
	avail_client: &OnlineClient<AvailConfig>,
) -> Result<u32, Box<dyn Error>> {
	let op_head_block = avail_client.rpc().block(None).await?;
	avail_client.offline();
	if let Some(head_block) = op_head_block {
		Ok(head_block.block.header.number)
	} else {
		Ok(0)
	}
}
pub fn spawn_query_block_task<T, B>(
	client: Arc<T>,
	task_manager: &TaskManager,
	avail_record: Arc<Mutex<AvailRecord>>,
) -> Result<(), Box<dyn Error>>
where
	B: BlockT,
	T: ProvideRuntimeApi<B>
		+ BlockOf
		+ AuxStore
		+ HeaderBackend<B>
		+ Send
		+ Sync
		+ 'static
		+ BlockBackend<B>,
	T::Api: AvailRuntimeApi<B>,
	<<B as BlockT>::Header as HeaderT>::Number: Into<u32>,
{
	task_manager.spawn_essential_handle().spawn("spawn_query_block", "magport", {
		// let lastest_hash = client.info().best_hash;
		// let bak_last_submit_block_confirm =
		// 	client.runtime_api().last_submit_block_confirm(lastest_hash)?;
		// let bak_last_avail_scan_block =
		// client.runtime_api().last_avail_scan_block(lastest_hash)?; let avail_record =
		// Arc::new(Mutex::new(AvailRecord { 	// last_submit_block_confirm:
		// bak_last_submit_block_confirm, 	// last_avail_scan_block: bak_last_avail_scan_block,
		// 	last_submit_block:1,
		// 	last_submit_block_confirm: 1,
		// 	last_avail_scan_block: 1,
		// }));
		async move {
			let avail_client =
				avail_subxt::build_client("ws://127.0.0.1:9945", false).await.unwrap();
			loop {
				//get Latest_finalized_block
				let latest_final_heght = client.info().finalized_number.into();
				let last_submit_block_confirm = {
					let avail_record_local = avail_record.lock().await;
					avail_record_local.last_submit_block_confirm
				};
				let last_avail_scan_block_confirm = {
					let avail_record_local = avail_record.lock().await;
					avail_record_local.last_avail_scan_block_confirm
				};
				// query solochain block is exist in avail
				let avail_latest_block_height =
					get_avail_latest_height(&avail_client).await.unwrap();
				log::info!(
					"================last_avail_scan_block_confirm:{:?}",
					last_avail_scan_block_confirm
				);
				log::info!(
					"================avail_latest_block_height:{:?}",
					avail_latest_block_height
				);

				// log::info!("last_submit_block_confirm:{:?}", last_submit_block_confirm);
				// log::info!("latest_final_heght:{:?}", latest_final_heght);
				let mut confirm_block_number = last_submit_block_confirm;
				let mut last_avail_scan_block = last_avail_scan_block_confirm;
				for block_number in last_avail_scan_block..=avail_latest_block_height {
					log::info!("================search avail block:{:?}========", block_number);
					for block_number_solo in last_submit_block_confirm + 1..=latest_final_heght {
						let find_result = query_block_exist(
							&avail_client,
							client.clone(),
							block_number,
							block_number_solo,
						)
						.await
						.unwrap();
						if find_result {
							confirm_block_number = block_number_solo;
							last_avail_scan_block = block_number;
							log::info!(
								"================find solo block:{:?}, result:{:?}========",
								block_number_solo,
								find_result
							);
						}
					}
				}
				{
					let mut avail_record_local = avail_record.lock().await;
					avail_record_local.last_submit_block_confirm = confirm_block_number;
					avail_record_local.last_avail_scan_block_confirm = last_avail_scan_block;
				}
				Delay::new(std::time::Duration::from_secs(6)).await;
			}
		}
	});
	Ok(())
}

pub fn spawn_submit_block_task<T, B>(
	client: Arc<T>,
	task_manager: &TaskManager,
	avail_record: Arc<Mutex<AvailRecord>>,
) -> Result<(), Box<dyn Error>>
where
	B: BlockT,
	T: ProvideRuntimeApi<B>
		+ BlockOf
		+ AuxStore
		+ HeaderBackend<B>
		+ Send
		+ Sync
		+ 'static
		+ BlockBackend<B>,
	T::Api: AvailRuntimeApi<B>,
	<<B as BlockT>::Header as HeaderT>::Number: Into<u32>,
{
	task_manager.spawn_essential_handle().spawn("spawn_submit_block", "magport", {
		async move {
			let avail_client =
				avail_subxt::build_client("ws://127.0.0.1:9945", false).await.unwrap();
			loop {
				let lastest_hash = client.info().best_hash;
				let latest_final_heght = client.info().finalized_number.into();
				// let bak_last_submit_block_confirm =
				// 		client.runtime_api().last_submit_block_confirm(lastest_hash).unwrap();
				let last_submit_block = {
					let avail_record_local = avail_record.lock().await;
					avail_record_local.last_submit_block
				};
				let last_submit_block_confirm = {
					let avail_record_local = avail_record.lock().await;
					avail_record_local.last_submit_block_confirm
				};
				log::info!(
					"================last_submit_block_confirm:{:?}",
					last_submit_block_confirm
				);
				log::info!("================last_submit_block:{:?}", last_submit_block);
				log::info!("================latest_final_heght:{:?}", latest_final_heght);
				// if last_submit_block_confirm < last_submit_block {
				// 	Delay::new(std::time::Duration::from_secs(6)).await;
				// 	continue;
				// }
				for block_number_solo in last_submit_block + 1..=latest_final_heght {
					let rollup_block_hash =
						client.block_hash(block_number_solo.into()).unwrap().unwrap();
					let rollup_block: Option<sp_runtime::generic::SignedBlock<B>> =
						BlockBackend::block(&*client, rollup_block_hash).unwrap();
					// log::info!("{:?}", rollup_block);
					match rollup_block {
						Some(block) => {
							let bytes = block.block.encode();
							let bytes = OtherBoundedVec(bytes);
							let data_transfer =
								AvailApi::tx().data_availability().submit_data(bytes.clone());
							let extrinsic_params =
								AvailExtrinsicParams::new_with_app_id(AppId(0u32));
							let signer = signer_from_seed("//Alice").unwrap();
							match avail_client
								.tx()
								.sign_and_submit(&data_transfer, &signer, extrinsic_params)
								.await
							{
								Ok(i) => log::info!(
									"================submit block:{:?},hash:{:?}",
									block_number_solo,
									i
								),
								Err(e) => {
									log::info!(
											"DA Layer error : failed due to closed websocket connection"
										)
								},
							};
						},
						None => {
							log::info!("None")
						},
					}
				}
				{
					let mut avail_record_local = avail_record.lock().await;
					avail_record_local.last_submit_block = latest_final_heght;
				}
				Delay::new(std::time::Duration::from_secs(6)).await;
			}
		}
	});
	Ok(())
}
