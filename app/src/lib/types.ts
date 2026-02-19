// Shared TypeScript types for the Ara Marketplace frontend

export type ContentType =
  | "game"
  | "music"
  | "video"
  | "document"
  | "software"
  | "other";

export interface ContentMetadata {
  title: string;
  description: string;
  contentType: ContentType;
  thumbnailUrl?: string;
  fileSizeBytes: number;
  fileName: string;
}

// Contract addresses (populated from config)
export const CONTRACTS = {
  ARA_TOKEN: "0xa92e7c82b11d10716ab534051b271d2f6aef7df5",
  ARA_STAKING: "", // Set after deployment
  CONTENT_REGISTRY: "", // Set after deployment
  MARKETPLACE: "", // Set after deployment
} as const;

export const CHAIN_ID = 1; // Ethereum mainnet
