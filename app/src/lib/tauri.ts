import { invoke } from "@tauri-apps/api/core";
import type { TransactionRequest } from "./transactions";

// Typed wrappers for Tauri IPC commands

// Wallet
export async function connectWallet(address: string): Promise<string> {
  return invoke("connect_wallet", { address });
}

export async function disconnectWallet(): Promise<void> {
  return invoke("disconnect_wallet");
}

export interface Balances {
  eth_balance: string;
  ara_balance: string;
  ara_staked: string;
  claimable_rewards: string;
}

export async function getBalances(): Promise<Balances> {
  return invoke("get_balances");
}

// Content

/** A single preview asset (image or video) stored as an iroh blob. */
export interface PreviewAsset {
  asset_type: "image" | "video";
  hash: string;
  filename: string;
  size: number;
}

/**
 * v2 metadata structure parsed from `metadata_uri` JSON.
 * All fields are optional — fall back gracefully for v1 content.
 */
export interface ContentMetadataV2 {
  v?: number;
  title?: string;
  description?: string;
  content_type?: string;
  filename?: string;
  file_size?: number;
  node_id?: string;
  relay_url?: string;
  categories?: string[];
  main_preview_image?: { hash: string; filename: string; size: number };
  main_preview_trailer?: { hash: string; filename: string; size: number };
  previews?: Array<{ type: string; hash: string; filename: string; size: number }>;
}

export interface ContentDetail {
  content_id: string;
  content_hash: string;
  creator: string;
  title: string;
  description: string;
  content_type: string;
  price_eth: string;
  active: boolean;
  seeder_count: number;
  purchase_count: number;
  /** Raw metadata_uri JSON — parse with JSON.parse() to get ContentMetadataV2 */
  metadata_uri: string;
  updated_at: number | null;
  categories: string[];
}

export interface PublishPrepareResult {
  content_hash: string;
  metadata_uri: string;
  transactions: TransactionRequest[];
}

export async function publishContent(params: {
  filePath: string;
  title: string;
  description: string;
  contentType: string;
  priceEth: string;
  categories?: string[];
  mainPreviewImagePath?: string;
  mainPreviewTrailerPath?: string;
  previewPaths?: string[];
}): Promise<PublishPrepareResult> {
  return invoke("publish_content", params);
}

export async function confirmPublish(
  contentHash: string,
  txHash: string
): Promise<void> {
  return invoke("confirm_publish", { contentHash, txHash });
}

export async function getContentDetail(
  contentId: string
): Promise<ContentDetail> {
  return invoke("get_content_detail", { contentId });
}

export async function searchContent(query: string): Promise<ContentDetail[]> {
  return invoke("search_content", { query });
}

export async function updateContent(params: {
  contentId: string;
  title: string;
  description: string;
  contentType: string;
  priceEth: string;
  categories?: string[];
}): Promise<TransactionRequest[]> {
  return invoke("update_content", params);
}

export async function confirmUpdateContent(params: {
  contentId: string;
  title: string;
  description: string;
  contentType: string;
  priceEth: string;
  categories?: string[];
}): Promise<void> {
  return invoke("confirm_update_content", params);
}

export async function getMyContent(): Promise<ContentDetail[]> {
  return invoke("get_my_content");
}

export interface PublishedItem {
  content_id: string;
  title: string;
  content_type: string;
  price_eth: string;
  is_seeding: boolean;
  file_size_bytes: number;
  updated_at: number | null;
}

export interface UpdateFileResult {
  new_content_hash: string;
  transactions: TransactionRequest[];
}

export async function updateContentFile(params: {
  contentId: string;
  filePath: string;
}): Promise<UpdateFileResult> {
  return invoke("update_content_file", params);
}

export async function confirmContentFileUpdate(params: {
  contentId: string;
  newContentHash: string;
}): Promise<void> {
  return invoke("confirm_content_file_update", params);
}

export async function importPreviewAssets(params: {
  filePaths: string[];
}): Promise<PreviewAsset[]> {
  return invoke("import_preview_assets", params);
}

export async function getPreviewAsset(params: {
  contentId: string;
  previewHash: string;
  filename: string;
}): Promise<string> {
  return invoke("get_preview_asset", params);
}

export async function getPublishedContent(): Promise<PublishedItem[]> {
  return invoke("get_published_content");
}

export async function delistContent(contentId: string): Promise<TransactionRequest[]> {
  return invoke("delist_content", { contentId });
}

export async function confirmDelist(contentId: string): Promise<void> {
  return invoke("confirm_delist", { contentId });
}

// Marketplace
export interface PurchasePrepareResult {
  content_id: string;
  title: string;
  price_eth: string;
  transactions: TransactionRequest[];
}

export async function purchaseContent(
  contentId: string
): Promise<PurchasePrepareResult> {
  return invoke("purchase_content", { contentId });
}

export interface ConfirmPurchaseResult {
  download_path: string;
}

export async function confirmPurchase(
  contentId: string,
  txHash: string
): Promise<ConfirmPurchaseResult> {
  return invoke("confirm_purchase", { contentId, txHash });
}

export interface LibraryItem {
  content_id: string;
  title: string;
  content_type: string;
  purchased_at: number;
  is_seeding: boolean;
  download_path: string | null;
  tx_hash: string | null;
}

export async function getLibrary(): Promise<LibraryItem[]> {
  return invoke("get_library");
}

export async function openDownloadedContent(contentId: string): Promise<string> {
  return invoke("open_downloaded_content", { contentId });
}

export async function openContentFolder(contentId: string): Promise<string> {
  return invoke("open_content_folder", { contentId });
}

// Seeding
export async function startSeeding(contentId: string): Promise<void> {
  return invoke("start_seeding", { contentId });
}

export async function stopSeeding(contentId: string): Promise<void> {
  return invoke("stop_seeding", { contentId });
}

export interface SeederStats {
  content_id: string;
  title: string;
  bytes_served: number;
  peer_count: number;
  ara_staked: string;
  is_active: boolean;
}

export async function getSeederStats(): Promise<SeederStats[]> {
  return invoke("get_seeder_stats");
}

// Staking — returns TransactionRequest[] for frontend signing
export async function stakeAra(
  amount: string
): Promise<TransactionRequest[]> {
  return invoke("stake_ara", { amount });
}

export async function unstakeAra(
  amount: string
): Promise<TransactionRequest[]> {
  return invoke("unstake_ara", { amount });
}

export async function stakeForContent(
  contentId: string,
  amount: string
): Promise<TransactionRequest[]> {
  return invoke("stake_for_content", { contentId, amount });
}

export interface StakeInfo {
  total_staked: string;
  general_balance: string;
  content_stakes: Array<{
    content_id: string;
    title: string;
    amount_staked: string;
    is_eligible_seeder: boolean;
  }>;
  claimable_rewards_eth: string;
}

export async function getStakeInfo(): Promise<StakeInfo> {
  return invoke("get_stake_info");
}

export async function claimRewards(): Promise<TransactionRequest[]> {
  return invoke("claim_rewards");
}

// Sync — pull content listings from on-chain events
export interface SyncResult {
  new_content: number;
  delisted_content: number;
  synced_to_block: number;
}

export async function syncContent(): Promise<SyncResult> {
  return invoke("sync_content");
}

// Reward distribution
export async function getMarketplaceAddress(): Promise<string> {
  return invoke("get_marketplace_address");
}

export async function broadcastDeliveryReceipt(params: {
  contentId: string;
  seederEthAddress: string;
  buyerEthAddress: string;
  signature: string;
  timestamp: number;
}): Promise<void> {
  return invoke("broadcast_delivery_receipt", params);
}

export async function getReceiptCount(contentId: string): Promise<number> {
  return invoke("get_receipt_count", { contentId });
}

export async function getRewardPool(contentId: string): Promise<string> {
  return invoke("get_reward_pool", { contentId });
}

export async function prepareDistributeRewards(
  contentId: string
): Promise<TransactionRequest[]> {
  return invoke("prepare_distribute_rewards", { contentId });
}

export async function preparePublicDistribute(
  contentId: string
): Promise<TransactionRequest[]> {
  return invoke("prepare_public_distribute", { contentId });
}
