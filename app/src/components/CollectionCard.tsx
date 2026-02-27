import { Link } from "react-router-dom";
import type { CollectionInfo, CollectionRanking } from "../lib/tauri";
import AddressDisplay from "./AddressDisplay";

type CardData = CollectionInfo | CollectionRanking;

interface CollectionCardProps {
  collection: CardData;
}

export default function CollectionCard({ collection }: CollectionCardProps) {
  const id = collection.collection_id;
  const floorPrice = "floor_price_eth" in collection ? collection.floor_price_eth : null;

  return (
    <Link
      to={`/collections/${id}`}
      className="card card-hover overflow-hidden group"
    >
      {/* Banner */}
      <div className="h-28 bg-gradient-to-br from-ara-600/30 to-purple-600/30 relative overflow-hidden">
        {collection.banner_uri && (
          <img
            src={collection.banner_uri}
            alt=""
            className="w-full h-full object-cover"
            onError={(e) => { (e.target as HTMLImageElement).style.display = "none"; }}
          />
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-black/40 to-transparent" />
      </div>

      {/* Info */}
      <div className="p-3">
        <h3 className="font-semibold text-sm truncate dark:text-white">
          {collection.name}
        </h3>
        <div className="text-xs text-gray-500 dark:text-gray-400 mt-1">
          <AddressDisplay address={collection.creator} />
        </div>
        <div className="flex items-center justify-between mt-2 text-xs text-gray-500 dark:text-gray-400">
          <span>{collection.item_count} items</span>
          {floorPrice && parseFloat(floorPrice) > 0 && (
            <span>Floor: {floorPrice} ETH</span>
          )}
        </div>
        {parseFloat(collection.volume_eth) > 0 && (
          <div className="text-xs text-gray-500 dark:text-gray-400 mt-1">
            Vol: {collection.volume_eth} ETH
          </div>
        )}
      </div>
    </Link>
  );
}
