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
use futures::{lock::Mutex, StreamExt};

use primitives_avail::{AvailRecord, AvailRuntimeApi};
use sc_client_api::{backend::AuxStore, BlockBackend, BlockOf, BlockchainEvents};
use sc_service::TaskManager;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_consensus::BlockOrigin;
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};
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
					let _rollup_block = node_template_runtime::Block::decode(&mut &data.0[..]);
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

async fn get_avail_latest_finalized_height(
	avail_client: &OnlineClient<AvailConfig>,
) -> Result<u32, Box<dyn Error>> {
	let latest_finalized_block_hash = avail_client.rpc().finalized_head().await?;
	let op_finalized_block = avail_client.rpc().block(Some(latest_finalized_block_hash)).await?;
	avail_client.offline();
	if let Some(head_block) = op_finalized_block {
		Ok(head_block.block.header.number)
	} else {
		Ok(0)
	}
}

pub fn spawn_avail_task<T, B>(
	client: Arc<T>,
	task_manager: &TaskManager,
	avail_record: Arc<Mutex<AvailRecord>>,
	avail_rpc_port: u16,
) -> Result<(), Box<dyn Error>>
where
	B: BlockT,
	T: BlockchainEvents<B>
		+ ProvideRuntimeApi<B>
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
		async move {
			let avail_url = format!("ws://127.0.0.1:{}", avail_rpc_port);
			log::info!("================ TASK | avail_url: {:?} ================", avail_url);
			let avail_client = Arc::new(Mutex::new(None::<OnlineClient<AvailConfig>>));
			let mut notification_st = client.import_notification_stream();
			while let Some(notification) = notification_st.next().await {
				if notification.origin != BlockOrigin::Own {
					continue;
				}
				// If inner of avail_client is None, try to connect avail
				if avail_client.lock().await.is_none() {
					let avail_client_inner = avail_subxt::build_client(&avail_url, false).await;
					if let Ok(avail_client_inner) = avail_client_inner {
						*avail_client.lock().await = Some(avail_client_inner);
						log::info!("================ TASK | avail service connected ================");
					} else {
						log::info!("================ TASK | avail service not connected ================");
						continue;
					}
				}
				// Get Avail Client
				let guard = avail_client.lock().await;
				let avail_client_ref = guard.as_ref().unwrap();
				
				let block_number: u32 = (*notification.header.number()).into();
				// Query
				if block_number % 5 == 0 {
					// Sync From Pallet
					let lastest_hash = client.info().best_hash;
					let last_submit_block_confirm = client.runtime_api().last_submit_block_confirm(lastest_hash).unwrap();
					let last_submit_block = client.runtime_api().last_submit_block(lastest_hash).unwrap();
					{
						let mut avail_record_local = avail_record.lock().await;
						log::info!(
							"================ QUERY TASK | before: avail_record: {:?} last_submit_block_confirm:{:?} last_submit_block:{:?} | pallet: last_submit_block_confirm:{:?}  last_submit_block:{:?} ================",
							avail_record_local.awaiting_inherent_processing,
							avail_record_local.last_submit_block_confirm,
							avail_record_local.last_submit_block,
							last_submit_block_confirm,
							last_submit_block,
						);
						avail_record_local.last_submit_block_confirm = last_submit_block_confirm;
						avail_record_local.last_submit_block = last_submit_block;
						avail_record_local.awaiting_inherent_processing = false;
					}

					let (last_submit_block_confirm, last_submit_block, last_avail_scan_block_confirm) = {
						let avail_record_local = avail_record.lock().await;
						log::info!(
							"================ QUERY TASK | after: avail_record last_submit_block_confirm:{:?} last_submit_block:{:?} ================",
							avail_record_local.last_submit_block_confirm,
							avail_record_local.last_submit_block
						);
						(avail_record_local.last_submit_block_confirm, avail_record_local.last_submit_block, avail_record_local.last_avail_scan_block_confirm)
					};

					if last_submit_block_confirm != last_submit_block {
						let avail_latest_finalized_height = get_avail_latest_finalized_height(avail_client_ref).await.unwrap();
						log::info!(
							"================ QUERY TASK | Avail Block Query Range: {:?} to {:?} ================",
							last_avail_scan_block_confirm,
							avail_latest_finalized_height
						);
						if last_avail_scan_block_confirm == avail_latest_finalized_height {
							log::info!("================ QUERY TASK | Avail's Finalized Block is the same as last_avail_scan_block_confirm, no need to query DA Layer ================");
							continue;
						}

						let mut confirm_block_number = last_submit_block_confirm;
						let mut last_avail_scan_block = last_avail_scan_block_confirm;
						for block_number in last_avail_scan_block_confirm..=avail_latest_finalized_height {
							log::info!("================ QUERY TASK | search avail block:{:?} ================", block_number);
							for block_number_solo in last_submit_block_confirm + 1..=last_submit_block {
								if let Ok(find_result) = query_block_exist(
									avail_client_ref,
									client.clone(),
									block_number,
									block_number_solo,
								)
								.await
								{
									log::info!(
										"================ QUERY TASK | solo block:{:?}, find result:{:?}========",
										block_number_solo,
										find_result
									);
									if !find_result {
										break;
									}
									confirm_block_number = block_number_solo;
									last_avail_scan_block = block_number;
								} else {
									log::info!(
										"Query task DA Layer error block_number: {:?} not found in DA Layer",
										block_number
									)
								}
							}
						}
						{
							let mut avail_record_local = avail_record.lock().await;
							avail_record_local.last_submit_block_confirm = confirm_block_number;
							avail_record_local.last_avail_scan_block_confirm = last_avail_scan_block;
							if confirm_block_number < last_submit_block {
								avail_record_local.last_submit_block = confirm_block_number;
								log::info!(
									"================ QUERY TASK | MISS BLOCK: {:?}, RESUBMIT SET last_submit_block_confirm: {:?} last_submit_block: {:?} last_avail_scan_block_confirm:{:?} ================",
									confirm_block_number+1,
									avail_record_local.last_submit_block_confirm,
									avail_record_local.last_submit_block,
									avail_record_local.last_avail_scan_block_confirm
								);
							} else {
								log::info!(
									"================ QUERY TASK | ALL FINALIZED Blocks Founded IN AVAIL: last_submit_block_confirm:{:?} last_avail_scan_block_confirm:{:?} ================",
									avail_record_local.last_submit_block_confirm,
									avail_record_local.last_avail_scan_block_confirm
								);
							}
						}
					} else {
						log::info!("================ QUERY TASK | last_submit_block_confirm == last_submit_block({:?}), no need to query DA Layer ================", last_submit_block_confirm);
					}
				}

				// Submit
				if block_number % 10 == 0 {
					log::info!("================ SUBMIT TASK | submit block task working: {:?} ================", block_number);
					let latest_final_height = client.info().finalized_number.into();

					let (last_submit_block, last_submit_block_confirm) = {
						let avail_record_local = avail_record.lock().await;
						(avail_record_local.last_submit_block, avail_record_local.last_submit_block_confirm)
					};

					if last_submit_block_confirm != last_submit_block {
						log::info!("================ SUBMIT TASK | Waiting Query Confirm Until last_submit_block_confirm: {:?} Equal To last_submit_block: {:?}, no need to submit to DA Layer ================", last_submit_block_confirm, last_submit_block);
						continue;
					}

					// log::info!("================ SUBMIT TASK | after: last_submit_block:{:?} ================", last_submit_block);
					log::info!("================ SUBMIT TASK | submit latest_final_height:{:?} ================", latest_final_height);
					for block_number_solo in last_submit_block + 1..=latest_final_height {
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
								match avail_client_ref
									.tx()
									.sign_and_submit(&data_transfer, &signer, extrinsic_params)
									.await
								{
									Ok(i) => log::info!(
										"================ SUBMIT TASK | submit block:{:?},hash:{:?} ================",
										block_number_solo,
										i
									),
									Err(_e) => {
										log::info!(
												"Submit task DA Layer error : failed due to closed websocket connection"
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
						avail_record_local.last_submit_block = latest_final_height;
					}
				}

				if block_number % 5 == 0 {
					{
						let mut avail_record_local = avail_record.lock().await;
						avail_record_local.awaiting_inherent_processing = true;
					}
				}
			}
		}
	});
	Ok(())
}
