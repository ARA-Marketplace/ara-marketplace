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

export async function confirmPurchase(
  contentId: string,
  txHash: string
): Promise<void> {
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
