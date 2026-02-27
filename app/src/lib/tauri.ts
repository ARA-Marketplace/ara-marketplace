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
  max_supply: number;
  total_minted: number;
  resale_count: number;
  min_resale_price_eth: string | null;
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
  maxSupply?: number;
  royaltyBps?: number;
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
  /** Unclaimed passive staker reward (ETH) */
  staker_reward_earned: string;
  /** Total stake used for reward weight */
  total_user_stake: string;
}

export async function getStakeInfo(): Promise<StakeInfo> {
  return invoke("get_stake_info");
}

export async function claimStakingReward(): Promise<TransactionRequest[]> {
  return invoke("claim_staking_reward");
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
  bytesServed: number;
}): Promise<void> {
  return invoke("broadcast_delivery_receipt", params);
}

export async function getReceiptCount(contentId: string): Promise<number> {
  return invoke("get_receipt_count", { contentId });
}

// Reward history and confirmation
export interface RewardHistoryItem {
  content_id: string;
  content_title: string;
  amount_eth: string;
  tx_hash: string | null;
  claimed: boolean;
  distributed_at: number;
}

export interface RewardHistoryResponse {
  items: RewardHistoryItem[];
  total_earned_eth: string;
  total_claimed_eth: string;
}

export async function getRewardHistory(
  limit?: number,
  offset?: number
): Promise<RewardHistoryResponse> {
  return invoke("get_reward_history", { limit, offset });
}

export async function confirmClaimRewards(txHash: string): Promise<void> {
  return invoke("confirm_claim_rewards", { txHash });
}

export interface RewardSyncResult {
  distributions_found: number;
  claims_found: number;
  purchases_found: number;
  synced_to_block: number;
}

export async function syncRewards(): Promise<RewardSyncResult> {
  return invoke("sync_rewards");
}

// Reward pipeline and one-click collect
export interface RewardPipelineResponse {
  available_eth: string;
  receipt_count: number;
  lifetime_earnings_eth: string;
}

export async function getRewardPipeline(): Promise<RewardPipelineResponse> {
  return invoke("get_reward_pipeline");
}

export async function prepareClaimRewards(): Promise<TransactionRequest[]> {
  return invoke("prepare_claim_rewards");
}

// Resale Marketplace

export interface ResaleListing {
  content_id: string;
  seller: string;
  price_eth: string;
  listed_at: number;
}

export interface EditionInfo {
  max_supply: number;
  total_minted: number;
  royalty_bps: number;
}

export interface BuyResalePrepareResult {
  content_id: string;
  title: string;
  price_eth: string;
  transactions: TransactionRequest[];
}

export async function listForResale(
  contentId: string,
  priceEth: string
): Promise<TransactionRequest[]> {
  return invoke("list_for_resale", { contentId, priceEth });
}

export async function confirmListForResale(
  contentId: string,
  priceEth: string
): Promise<void> {
  return invoke("confirm_list_for_resale", { contentId, priceEth });
}

export async function cancelResaleListing(
  contentId: string
): Promise<TransactionRequest[]> {
  return invoke("cancel_resale_listing", { contentId });
}

export async function confirmCancelListing(
  contentId: string
): Promise<void> {
  return invoke("confirm_cancel_listing", { contentId });
}

export async function buyResale(
  contentId: string,
  seller: string
): Promise<BuyResalePrepareResult> {
  return invoke("buy_resale", { contentId, seller });
}

export async function getResaleListings(
  contentId: string
): Promise<ResaleListing[]> {
  return invoke("get_resale_listings", { contentId });
}

export async function getEditionInfo(
  contentId: string
): Promise<EditionInfo> {
  return invoke("get_edition_info", { contentId });
}

// Collections

export interface CollectionInfo {
  collection_id: number;
  creator: string;
  name: string;
  description: string;
  banner_uri: string;
  item_count: number;
  volume_eth: string;
  created_at: number;
}

export interface CollectionRanking {
  collection_id: number;
  name: string;
  creator: string;
  banner_uri: string;
  floor_price_eth: string;
  item_count: number;
  volume_eth: string;
}

export async function createCollection(params: {
  name: string;
  description: string;
  bannerUri: string;
}): Promise<TransactionRequest[]> {
  return invoke("create_collection", params);
}

export async function confirmCreateCollection(params: {
  txHash: string;
  name: string;
  description: string;
  bannerUri: string;
}): Promise<number> {
  return invoke("confirm_create_collection", params);
}

export async function updateCollection(params: {
  collectionId: number;
  name: string;
  description: string;
  bannerUri: string;
}): Promise<TransactionRequest[]> {
  return invoke("update_collection", params);
}

export async function confirmUpdateCollection(params: {
  collectionId: number;
  name: string;
  description: string;
  bannerUri: string;
}): Promise<void> {
  return invoke("confirm_update_collection", params);
}

export async function deleteCollection(
  collectionId: number
): Promise<TransactionRequest[]> {
  return invoke("delete_collection", { collectionId });
}

export async function confirmDeleteCollection(
  collectionId: number
): Promise<void> {
  return invoke("confirm_delete_collection", { collectionId });
}

export async function addToCollection(
  collectionId: number,
  contentId: string
): Promise<TransactionRequest[]> {
  return invoke("add_to_collection", { collectionId, contentId });
}

export async function confirmAddToCollection(
  collectionId: number,
  contentId: string
): Promise<void> {
  return invoke("confirm_add_to_collection", { collectionId, contentId });
}

export async function removeFromCollection(
  collectionId: number,
  contentId: string
): Promise<TransactionRequest[]> {
  return invoke("remove_from_collection", { collectionId, contentId });
}

export async function confirmRemoveFromCollection(
  collectionId: number,
  contentId: string
): Promise<void> {
  return invoke("confirm_remove_from_collection", { collectionId, contentId });
}

export async function getMyCollections(): Promise<CollectionInfo[]> {
  return invoke("get_my_collections");
}

export async function getCollection(
  collectionId: number
): Promise<CollectionInfo> {
  return invoke("get_collection", { collectionId });
}

export async function getCollectionItems(
  collectionId: number
): Promise<string[]> {
  return invoke("get_collection_items", { collectionId });
}

export async function getAllCollections(
  limit?: number,
  offset?: number
): Promise<CollectionInfo[]> {
  return invoke("get_all_collections", { limit, offset });
}

export async function getContentCollection(
  contentId: string
): Promise<number | null> {
  return invoke("get_content_collection", { contentId });
}

export async function getTopCollections(
  limit?: number
): Promise<CollectionRanking[]> {
  return invoke("get_top_collections", { limit });
}

// Name Registry

export async function registerName(
  name: string
): Promise<TransactionRequest[]> {
  return invoke("register_name", { name });
}

export async function confirmRegisterName(name: string): Promise<void> {
  return invoke("confirm_register_name", { name });
}

export async function removeDisplayName(): Promise<TransactionRequest[]> {
  return invoke("remove_display_name");
}

export async function confirmRemoveName(): Promise<void> {
  return invoke("confirm_remove_name");
}

export async function getDisplayName(
  address: string
): Promise<string | null> {
  return invoke("get_display_name", { address });
}

export async function getDisplayNames(
  addresses: string[]
): Promise<Record<string, string>> {
  return invoke("get_display_names", { addresses });
}

// Analytics

export interface PricePoint {
  price_eth: string;
  block_number: number;
  buyer: string;
  tx_hash: string;
  is_resale: boolean;
}

export interface ItemAnalytics {
  total_sales: number;
  total_volume_eth: string;
  unique_buyers: number;
}

export interface CollectorRanking {
  address: string;
  purchase_count: number;
  total_spent_eth: string;
}

export interface TrendingItem {
  content_id: string;
  recent_sales: number;
  title: string;
  price_eth: string;
  content_type: string;
}

export interface MarketplaceOverview {
  total_volume_eth: string;
  total_sales: number;
  total_collections: number;
  total_items: number;
}

export async function getPriceHistory(
  contentId: string
): Promise<PricePoint[]> {
  return invoke("get_price_history", { contentId });
}

export async function getItemAnalytics(
  contentId: string
): Promise<ItemAnalytics> {
  return invoke("get_item_analytics", { contentId });
}

export async function getTopCollectors(
  limit?: number
): Promise<CollectorRanking[]> {
  return invoke("get_top_collectors", { limit });
}

export async function getTrendingContent(
  limit?: number
): Promise<TrendingItem[]> {
  return invoke("get_trending_content", { limit });
}

export async function getMarketplaceOverview(): Promise<MarketplaceOverview> {
  return invoke("get_marketplace_overview");
}
