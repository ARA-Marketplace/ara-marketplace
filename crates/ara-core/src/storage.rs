use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

/// A buyer-signed proof that a seeder served them specific content.
#[derive(Debug, Clone)]
pub struct DeliveryReceipt {
    pub content_id: String,
    pub seeder_eth_address: String,
    pub buyer_eth_address: String,
    /// Hex-encoded 65-byte EIP-712 ECDSA signature
    pub signature: String,
    pub timestamp: i64,
    /// Number of bytes this seeder delivered (for proportional reward claiming)
    pub bytes_served: u64,
}

/// A reward event row (distribution or claim) from the local DB cache.
#[derive(Debug, Clone)]
pub struct RewardRow {
    pub id: i64,
    pub content_id: String,
    pub amount_wei: String,
    pub tx_hash: Option<String>,
    pub claimed: bool,
    pub distributed_at: i64,
    pub content_title: Option<String>,
}

/// Local SQLite database for caching on-chain state, seeding metrics, and user data.
pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS content (
                content_id TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                creator TEXT NOT NULL,
                metadata_uri TEXT NOT NULL,
                price_wei TEXT NOT NULL,
                title TEXT,
                description TEXT,
                content_type TEXT,
                thumbnail_url TEXT,
                file_size_bytes INTEGER,
                active INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                publisher_node_id TEXT,
                publisher_relay_url TEXT,
                filename TEXT,
                updated_at INTEGER,
                categories TEXT
            );

            CREATE TABLE IF NOT EXISTS purchases (
                content_id TEXT NOT NULL,
                buyer TEXT NOT NULL,
                price_paid_wei TEXT NOT NULL,
                tx_hash TEXT,
                purchased_at INTEGER NOT NULL,
                downloaded_path TEXT,
                PRIMARY KEY (content_id, buyer)
            );

            CREATE TABLE IF NOT EXISTS seeding (
                content_id TEXT PRIMARY KEY,
                active INTEGER NOT NULL DEFAULT 1,
                bytes_served INTEGER NOT NULL DEFAULT 0,
                peer_count INTEGER NOT NULL DEFAULT 0,
                started_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS rewards (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content_id TEXT NOT NULL,
                amount_wei TEXT NOT NULL,
                tx_hash TEXT,
                claimed INTEGER NOT NULL DEFAULT 0,
                distributed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS content_seeders (
                content_hash TEXT NOT NULL,
                node_id TEXT NOT NULL,
                eth_address TEXT,
                discovered_at INTEGER NOT NULL,
                PRIMARY KEY (content_hash, node_id)
            );

            CREATE TABLE IF NOT EXISTS delivery_receipts (
                content_id TEXT NOT NULL,
                seeder_eth_address TEXT NOT NULL,
                buyer_eth_address TEXT NOT NULL,
                signature TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                bytes_served INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (content_id, seeder_eth_address, buyer_eth_address)
            );

            CREATE TABLE IF NOT EXISTS resale_listings (
                content_id TEXT NOT NULL,
                seller TEXT NOT NULL,
                price_wei TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                listed_at INTEGER NOT NULL,
                PRIMARY KEY (content_id, seller)
            );
            ",
        )?;

        // Incremental migrations for existing databases (columns may already exist)
        let _ = self
            .conn
            .execute("ALTER TABLE content ADD COLUMN publisher_node_id TEXT", []);
        let _ = self
            .conn
            .execute("ALTER TABLE content ADD COLUMN filename TEXT", []);
        let _ = self
            .conn
            .execute("ALTER TABLE content ADD COLUMN publisher_relay_url TEXT", []);
        let _ = self
            .conn
            .execute("ALTER TABLE purchases ADD COLUMN downloaded_path TEXT", []);
        // content_seeders is a new table — CREATE TABLE IF NOT EXISTS handles existing DBs
        // Add eth_address column to content_seeders if not present (existing DBs)
        let _ = self
            .conn
            .execute("ALTER TABLE content_seeders ADD COLUMN eth_address TEXT", []);
        // delivery_receipts is a new table — CREATE TABLE IF NOT EXISTS handles existing DBs
        // Migrate: old unique index on tx_hash alone prevented multiple events per tx.
        // Replace with (content_id, tx_hash) so batch claims record every content.
        let _ = self.conn.execute("DROP INDEX IF EXISTS idx_rewards_tx_hash", []);
        let _ = self.conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_rewards_content_tx ON rewards(content_id, tx_hash)",
            [],
        );
        // Migrate: old code inserted rewards with claimed=0 even for on-chain claims.
        // In the per-receipt model every recorded reward is a completed claim.
        let _ = self.conn.execute("UPDATE rewards SET claimed = 1 WHERE claimed = 0", []);
        // Reset rewards sync checkpoint so the background sync re-processes claim events
        // with the corrected index and claimed flag. Idempotent: only fires once because
        // subsequent syncs will re-insert rows that INSERT OR IGNORE will skip.
        let _ = self.conn.execute(
            "DELETE FROM config WHERE key = 'rewards_sync_block'",
            [],
        );
        // updated_at and categories are new columns — silently ignored if already present
        let _ = self
            .conn
            .execute("ALTER TABLE content ADD COLUMN updated_at INTEGER", []);
        let _ = self
            .conn
            .execute("ALTER TABLE content ADD COLUMN categories TEXT", []);
        // bytes_served column for delivery receipts — silently ignored if already present
        let _ = self
            .conn
            .execute("ALTER TABLE delivery_receipts ADD COLUMN bytes_served INTEGER NOT NULL DEFAULT 0", []);
        // ERC-1155 edition columns — silently ignored if already present
        let _ = self
            .conn
            .execute("ALTER TABLE content ADD COLUMN max_supply INTEGER NOT NULL DEFAULT 0", []);
        let _ = self
            .conn
            .execute("ALTER TABLE content ADD COLUMN royalty_bps INTEGER NOT NULL DEFAULT 0", []);
        let _ = self
            .conn
            .execute("ALTER TABLE content ADD COLUMN total_minted INTEGER NOT NULL DEFAULT 0", []);

        // Collections tables
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS collections (
                collection_id INTEGER PRIMARY KEY,
                creator TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                banner_uri TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS collection_items (
                collection_id INTEGER NOT NULL,
                content_id TEXT NOT NULL,
                added_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (collection_id, content_id)
            );

            CREATE TABLE IF NOT EXISTS address_names (
                address TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS all_purchases (
                content_id TEXT NOT NULL,
                buyer TEXT NOT NULL,
                seller TEXT,
                price_paid_wei TEXT NOT NULL,
                tx_hash TEXT NOT NULL,
                block_number INTEGER NOT NULL,
                timestamp INTEGER,
                is_resale INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (tx_hash, content_id)
            );
            ",
        )?;

        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Get a config value by key.
    pub fn get_config(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM config WHERE key = ?1",
                rusqlite::params![key],
                |row| row.get(0),
            )
            .ok()
    }

    /// Set a config value (upsert).
    pub fn set_config(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    /// Record that a seeder's Ethereum address is known (from SeederIdentity gossip).
    pub fn set_seeder_eth_address(
        &self,
        content_hash: &str,
        node_id: &str,
        eth_address: &str,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE content_seeders SET eth_address = ?1
             WHERE content_hash = ?2 AND node_id = ?3",
            rusqlite::params![eth_address, content_hash, node_id],
        )
    }

    /// Store a buyer-signed delivery receipt attesting that a seeder served them content.
    /// Idempotent: silently ignores duplicate (content_id, seeder, buyer) triples.
    pub fn insert_delivery_receipt(
        &self,
        content_id: &str,
        seeder_eth_address: &str,
        buyer_eth_address: &str,
        signature: &str,
        timestamp: i64,
        bytes_served: u64,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "INSERT OR IGNORE INTO delivery_receipts
             (content_id, seeder_eth_address, buyer_eth_address, signature, timestamp, bytes_served)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![content_id, seeder_eth_address, buyer_eth_address, signature, timestamp, bytes_served as i64],
        )
    }

    /// Get all delivery receipts for a content item.
    pub fn get_receipts_for_content(&self, content_id: &str) -> Result<Vec<DeliveryReceipt>> {
        let mut stmt = self.conn.prepare(
            "SELECT seeder_eth_address, buyer_eth_address, signature, timestamp, bytes_served
             FROM delivery_receipts WHERE content_id = ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![content_id], |row| {
            Ok(DeliveryReceipt {
                content_id: content_id.to_string(),
                seeder_eth_address: row.get(0)?,
                buyer_eth_address: row.get(1)?,
                signature: row.get(2)?,
                timestamp: row.get(3)?,
                bytes_served: row.get::<_, i64>(4)? as u64,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Get all delivery receipts where the given address is the seeder.
    pub fn get_receipts_for_seeder(&self, seeder_eth_address: &str) -> Result<Vec<DeliveryReceipt>> {
        let mut stmt = self.conn.prepare(
            "SELECT content_id, seeder_eth_address, buyer_eth_address, signature, timestamp, bytes_served
             FROM delivery_receipts WHERE LOWER(seeder_eth_address) = LOWER(?1)",
        )?;
        let rows = stmt.query_map(rusqlite::params![seeder_eth_address], |row| {
            Ok(DeliveryReceipt {
                content_id: row.get(0)?,
                seeder_eth_address: row.get(1)?,
                buyer_eth_address: row.get(2)?,
                signature: row.get(3)?,
                timestamp: row.get(4)?,
                bytes_served: row.get::<_, i64>(5)? as u64,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Count verified delivery receipts per seeder for a content item.
    /// Returns a map of seeder_eth_address → receipt count.
    pub fn count_receipts_per_seeder(&self, content_id: &str) -> Result<HashMap<String, u64>> {
        let mut stmt = self.conn.prepare(
            "SELECT seeder_eth_address, COUNT(*) FROM delivery_receipts
             WHERE content_id = ?1
             GROUP BY seeder_eth_address",
        )?;
        let rows = stmt.query_map(rusqlite::params![content_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (seeder, count) = row?;
            map.insert(seeder, count);
        }
        Ok(map)
    }

    // ── Reward tracking ──

    /// Insert a reward claim record. Deduplicates by (content_id, tx_hash).
    /// In the per-receipt model every recorded reward is an on-chain claim,
    /// so `claimed` is always 1.
    pub fn insert_reward(
        &self,
        content_id: &str,
        amount_wei: &str,
        tx_hash: &str,
        distributed_at: i64,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "INSERT OR IGNORE INTO rewards (content_id, amount_wei, tx_hash, claimed, distributed_at)
             VALUES (?1, ?2, ?3, 1, ?4)",
            rusqlite::params![content_id, amount_wei, tx_hash, distributed_at],
        )
    }

    /// Insert a reward claim record (already claimed on-chain).
    pub fn insert_reward_claim(
        &self,
        amount_wei: &str,
        tx_hash: &str,
        claimed_at: i64,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "INSERT OR IGNORE INTO rewards (content_id, amount_wei, tx_hash, claimed, distributed_at)
             VALUES ('claim', ?1, ?2, 1, ?3)",
            rusqlite::params![amount_wei, tx_hash, claimed_at],
        )
    }

    /// Get reward history with pagination (newest first).
    pub fn get_reward_history(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<RewardRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.content_id, r.amount_wei, r.tx_hash, r.claimed, r.distributed_at,
                    c.title
             FROM rewards r
             LEFT JOIN content c ON r.content_id = c.content_id
             ORDER BY r.distributed_at DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit, offset], |row| {
            Ok(RewardRow {
                id: row.get(0)?,
                content_id: row.get(1)?,
                amount_wei: row.get(2)?,
                tx_hash: row.get(3)?,
                claimed: row.get::<_, i32>(4)? != 0,
                distributed_at: row.get(5)?,
                content_title: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Get total ETH claimed (sum of amount_wei where claimed = 1), as a string.
    pub fn get_total_claimed_wei(&self) -> Result<String> {
        // SUM() returns an integer in SQLite; read as i64 then convert to string
        // (rusqlite's FromSql for String only handles Text, not Integer).
        let total: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(CAST(amount_wei AS INTEGER)), 0) FROM rewards WHERE claimed = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(total.to_string())
    }

    /// Get total ETH distributed but not yet claimed (claimed = 0), as a string.
    pub fn get_total_unclaimed_wei(&self) -> Result<String> {
        let total: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(CAST(amount_wei AS INTEGER)), 0) FROM rewards WHERE claimed = 0",
            [],
            |row| row.get(0),
        )?;
        Ok(total.to_string())
    }

    /// Upsert a purchase row from on-chain event sync.
    pub fn upsert_purchase(
        &self,
        content_id: &str,
        buyer: &str,
        price_paid_wei: &str,
        tx_hash: &str,
        purchased_at: i64,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "INSERT OR IGNORE INTO purchases (content_id, buyer, price_paid_wei, tx_hash, purchased_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![content_id, buyer, price_paid_wei, tx_hash, purchased_at],
        )
    }

    /// Increment the total_minted counter for a content item (called on ContentPurchased).
    pub fn increment_total_minted(&self, content_id: &str) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE content SET total_minted = total_minted + 1 WHERE content_id = ?1",
            rusqlite::params![content_id],
        )
    }

    // ── Resale listings ──

    /// Upsert a resale listing (from on-chain event sync or local confirm).
    pub fn upsert_resale_listing(
        &self,
        content_id: &str,
        seller: &str,
        price_wei: &str,
        listed_at: i64,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "INSERT OR REPLACE INTO resale_listings
             (content_id, seller, price_wei, active, listed_at)
             VALUES (?1, ?2, ?3, 1, ?4)",
            rusqlite::params![content_id, seller, price_wei, listed_at],
        )
    }

    /// Mark a resale listing as inactive (cancelled or sold).
    pub fn deactivate_resale_listing(
        &self,
        content_id: &str,
        seller: &str,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE resale_listings SET active = 0
             WHERE content_id = ?1 AND LOWER(seller) = LOWER(?2)",
            rusqlite::params![content_id, seller],
        )
    }

    /// Get all active resale listings for a content item, sorted by price ascending.
    pub fn get_active_resale_listings(
        &self,
        content_id: &str,
    ) -> Result<Vec<(String, String, String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT content_id, seller, price_wei, listed_at
             FROM resale_listings
             WHERE content_id = ?1 AND active = 1
             ORDER BY CAST(price_wei AS INTEGER) ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![content_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Upsert a content row discovered from on-chain event sync.
    /// On conflict, updates price and metadata but preserves local publisher data.
    /// `categories` is a JSON array string (e.g. `["action","indie"]`); pass `""` if unknown.
    pub fn upsert_synced_content(
        &self,
        content_id: &str,
        content_hash: &str,
        creator: &str,
        metadata_uri: &str,
        price_wei: &str,
        title: &str,
        description: &str,
        content_type: &str,
        filename: &str,
        file_size: i64,
        publisher_node_id: &str,
        publisher_relay_url: &str,
        created_at: i64,
        categories: &str,
        max_supply: i64,
        royalty_bps: i64,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "INSERT INTO content
             (content_id, content_hash, creator, metadata_uri, price_wei,
              title, description, content_type, file_size_bytes, active,
              created_at, publisher_node_id, publisher_relay_url, filename, categories,
              max_supply, royalty_bps)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(content_id) DO UPDATE SET
               price_wei = excluded.price_wei,
               metadata_uri = excluded.metadata_uri,
               title = CASE WHEN excluded.title != '' THEN excluded.title ELSE title END,
               description = CASE WHEN excluded.description != '' THEN excluded.description ELSE description END,
               content_type = CASE WHEN excluded.content_type != '' THEN excluded.content_type ELSE content_type END,
               filename = CASE WHEN excluded.filename != '' THEN excluded.filename ELSE filename END,
               file_size_bytes = CASE WHEN excluded.file_size_bytes > 0 THEN excluded.file_size_bytes ELSE file_size_bytes END,
               publisher_node_id = CASE WHEN excluded.publisher_node_id != '' THEN excluded.publisher_node_id ELSE publisher_node_id END,
               publisher_relay_url = CASE WHEN excluded.publisher_relay_url != '' THEN excluded.publisher_relay_url ELSE publisher_relay_url END,
               categories = CASE WHEN excluded.categories != '' THEN excluded.categories ELSE categories END,
               max_supply = CASE WHEN excluded.max_supply > 0 THEN excluded.max_supply ELSE max_supply END,
               royalty_bps = CASE WHEN excluded.royalty_bps > 0 THEN excluded.royalty_bps ELSE royalty_bps END,
               active = 1",
            rusqlite::params![
                content_id, content_hash, creator, metadata_uri, price_wei,
                title, description, content_type, file_size, created_at,
                publisher_node_id, publisher_relay_url, filename, categories,
                max_supply, royalty_bps,
            ],
        )
    }

    // === Collections ===

    pub fn upsert_collection(
        &self,
        collection_id: i64,
        creator: &str,
        name: &str,
        description: &str,
        banner_uri: &str,
        created_at: i64,
        active: bool,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO collections (collection_id, creator, name, description, banner_uri, created_at, active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(collection_id) DO UPDATE SET
               name = excluded.name,
               description = excluded.description,
               banner_uri = excluded.banner_uri,
               active = excluded.active",
            rusqlite::params![collection_id, creator, name, description, banner_uri, created_at, active as i32],
        )?;
        Ok(())
    }

    pub fn upsert_collection_item(&self, collection_id: i64, content_id: &str, added_at: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO collection_items (collection_id, content_id, added_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![collection_id, content_id, added_at],
        )?;
        Ok(())
    }

    pub fn remove_collection_item(&self, collection_id: i64, content_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM collection_items WHERE collection_id = ?1 AND content_id = ?2",
            rusqlite::params![collection_id, content_id],
        )?;
        Ok(())
    }

    pub fn delete_collection_items(&self, collection_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM collection_items WHERE collection_id = ?1",
            rusqlite::params![collection_id],
        )?;
        Ok(())
    }

    pub fn get_all_collections(&self, limit: u32, offset: u32) -> Result<Vec<(i64, String, String, String, String, i64, bool, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.collection_id, c.creator, c.name, c.description, c.banner_uri, c.created_at, c.active,
                    (SELECT COUNT(*) FROM collection_items ci WHERE ci.collection_id = c.collection_id) as item_count
             FROM collections c
             WHERE c.active = 1
             ORDER BY c.created_at DESC
             LIMIT ?1 OFFSET ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit, offset], |row| {
            Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                row.get(4)?, row.get(5)?, row.get::<_, i32>(6)? != 0, row.get::<_, u32>(7)?,
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_collections_by_creator(&self, creator: &str) -> Result<Vec<(i64, String, String, String, i64, bool, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.collection_id, c.name, c.description, c.banner_uri, c.created_at, c.active,
                    (SELECT COUNT(*) FROM collection_items ci WHERE ci.collection_id = c.collection_id) as item_count
             FROM collections c
             WHERE c.creator = ?1 AND c.active = 1
             ORDER BY c.created_at DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![creator], |row| {
            Ok((
                row.get(0)?, row.get(1)?, row.get(2)?,
                row.get(3)?, row.get(4)?, row.get::<_, i32>(5)? != 0, row.get::<_, u32>(6)?,
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_collection_item_ids(&self, collection_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT content_id FROM collection_items WHERE collection_id = ?1 ORDER BY added_at"
        )?;
        let rows = stmt.query_map(rusqlite::params![collection_id], |row| row.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_content_collection_id(&self, content_id: &str) -> Result<Option<i64>> {
        let result = self.conn.query_row(
            "SELECT collection_id FROM collection_items WHERE content_id = ?1",
            rusqlite::params![content_id],
            |row| row.get(0),
        );
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // === Address Names ===

    pub fn upsert_name(&self, address: &str, display_name: &str, updated_at: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO address_names (address, display_name, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(address) DO UPDATE SET display_name = excluded.display_name, updated_at = excluded.updated_at",
            rusqlite::params![address, display_name, updated_at],
        )?;
        Ok(())
    }

    pub fn remove_name(&self, address: &str) -> Result<()> {
        self.conn.execute("DELETE FROM address_names WHERE address = ?1", rusqlite::params![address])?;
        Ok(())
    }

    pub fn get_name(&self, address: &str) -> Option<String> {
        self.conn.query_row(
            "SELECT display_name FROM address_names WHERE address = ?1",
            rusqlite::params![address],
            |row| row.get(0),
        ).ok()
    }

    pub fn get_names_batch(&self, addresses: &[&str]) -> HashMap<String, String> {
        let mut result = HashMap::new();
        for addr in addresses {
            if let Some(name) = self.get_name(addr) {
                result.insert(addr.to_string(), name);
            }
        }
        result
    }

    // === All Purchases (for analytics) ===

    pub fn record_global_purchase(&self, content_id: &str, buyer: &str, seller: Option<&str>, price_paid_wei: &str, tx_hash: &str, block_number: i64, timestamp: Option<i64>, is_resale: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO all_purchases (content_id, buyer, seller, price_paid_wei, tx_hash, block_number, timestamp, is_resale)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![content_id, buyer, seller, price_paid_wei, tx_hash, block_number, timestamp, is_resale as i32],
        )?;
        Ok(())
    }

    pub fn get_price_history(&self, content_id: &str) -> Result<Vec<(String, i64, String, String, bool)>> {
        let mut stmt = self.conn.prepare(
            "SELECT price_paid_wei, COALESCE(timestamp, block_number), buyer, tx_hash, is_resale
             FROM all_purchases WHERE content_id = ?1 ORDER BY block_number ASC"
        )?;
        let rows = stmt.query_map(rusqlite::params![content_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get::<_, i32>(4)? != 0))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_top_collectors(&self, limit: u32) -> Result<Vec<(String, u32, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT buyer, COUNT(*) as cnt, CAST(SUM(CAST(price_paid_wei AS INTEGER)) AS TEXT) as total
             FROM all_purchases GROUP BY buyer ORDER BY total DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2).unwrap_or_default()))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_trending_content(&self, limit: u32, since_block: i64) -> Result<Vec<(String, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT content_id, COUNT(*) as cnt FROM all_purchases
             WHERE block_number >= ?1 GROUP BY content_id ORDER BY cnt DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![since_block, limit], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_collection_volume(&self, collection_id: i64) -> Result<String> {
        let result: String = self.conn.query_row(
            "SELECT COALESCE(CAST(SUM(CAST(ap.price_paid_wei AS INTEGER)) AS TEXT), '0')
             FROM all_purchases ap
             JOIN collection_items ci ON ci.content_id = ap.content_id
             WHERE ci.collection_id = ?1",
            rusqlite::params![collection_id],
            |row| row.get(0),
        ).unwrap_or_else(|_| "0".to_string());
        Ok(result)
    }

    pub fn get_top_collections(&self, limit: u32) -> Result<Vec<(i64, String, String, String, String, u32, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.collection_id, c.name, c.creator, c.banner_uri,
                    COALESCE(MIN(ct.price_wei), '0') as floor_price,
                    COUNT(DISTINCT ci.content_id) as item_count,
                    COALESCE(CAST(SUM(CAST(ap.price_paid_wei AS INTEGER)) AS TEXT), '0') as volume
             FROM collections c
             LEFT JOIN collection_items ci ON ci.collection_id = c.collection_id
             LEFT JOIN content ct ON ct.content_id = ci.content_id AND ct.active = 1
             LEFT JOIN all_purchases ap ON ap.content_id = ci.content_id
             WHERE c.active = 1
             GROUP BY c.collection_id
             ORDER BY volume DESC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.conn().is_autocommit());
    }

    #[test]
    fn test_collections_crud() {
        let db = Database::open_in_memory().unwrap();

        // Create a collection
        db.upsert_collection(1, "0xabc", "My Collection", "A test collection", "https://banner.png", 1000, true).unwrap();

        // Verify it exists in all_collections
        let all = db.get_all_collections(10, 0).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, 1); // collection_id
        assert_eq!(all[0].2, "My Collection"); // name

        // Get by creator
        let by_creator = db.get_collections_by_creator("0xabc").unwrap();
        assert_eq!(by_creator.len(), 1);
        assert_eq!(by_creator[0].1, "My Collection");

        // Add items to collection
        db.upsert_collection_item(1, "content_001", 1001).unwrap();
        db.upsert_collection_item(1, "content_002", 1002).unwrap();

        // Update collection
        db.upsert_collection(1, "0xabc", "Updated Collection", "New desc", "https://new-banner.png", 1000, true).unwrap();
        let updated = db.get_all_collections(10, 0).unwrap();
        assert_eq!(updated[0].2, "Updated Collection");

        // Remove item from collection
        db.remove_collection_item(1, "content_001").unwrap();

        // Delete collection (set inactive)
        db.upsert_collection(1, "0xabc", "Updated Collection", "New desc", "https://new-banner.png", 1000, false).unwrap();
        // All collections filters by active=1
        let active = db.get_all_collections(10, 0).unwrap();
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn test_address_names() {
        let db = Database::open_in_memory().unwrap();

        // No name initially
        assert!(db.get_name("0xabc").is_none());

        // Register a name
        db.upsert_name("0xabc", "alice", 1000).unwrap();
        assert_eq!(db.get_name("0xabc"), Some("alice".to_string()));

        // Update name
        db.upsert_name("0xabc", "alice-v2", 2000).unwrap();
        assert_eq!(db.get_name("0xabc"), Some("alice-v2".to_string()));

        // Batch lookup
        db.upsert_name("0xdef", "bob", 1000).unwrap();
        let names = db.get_names_batch(&["0xabc", "0xdef", "0xunknown"]);
        assert_eq!(names.len(), 2);
        assert_eq!(names.get("0xabc"), Some(&"alice-v2".to_string()));
        assert_eq!(names.get("0xdef"), Some(&"bob".to_string()));
        assert!(names.get("0xunknown").is_none());
    }

    #[test]
    fn test_global_purchases_and_analytics() {
        let db = Database::open_in_memory().unwrap();

        // Insert some content first (needed for trending/price history)
        db.conn().execute(
            "INSERT INTO content (content_id, content_hash, creator, metadata_uri, price_wei, title, content_type, active, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 1000)",
            rusqlite::params!["cid_001", "hash_001", "0xcreator", "{}", "1000000000000000", "Test Song", "music"],
        ).unwrap();

        // Record purchases
        db.record_global_purchase("cid_001", "0xbuyer1", None, "1000000000000000", "0xtx1", 100, Some(1000), false).unwrap();
        db.record_global_purchase("cid_001", "0xbuyer2", None, "1000000000000000", "0xtx2", 101, Some(1001), false).unwrap();
        db.record_global_purchase("cid_001", "0xbuyer3", Some("0xbuyer1"), "2000000000000000", "0xtx3", 102, Some(1002), true).unwrap();

        // Price history
        let history = db.get_price_history("cid_001").unwrap();
        assert_eq!(history.len(), 3);
        assert!(!history[0].4); // first is not resale
        assert!(history[2].4); // last is resale

        // Top collectors
        let collectors = db.get_top_collectors(10).unwrap();
        assert!(!collectors.is_empty());
        // buyer1 and buyer2 each bought once, buyer3 bought resale — all should appear
        let total_purchases: u32 = collectors.iter().map(|c| c.1).sum();
        assert_eq!(total_purchases, 3);

        // Trending content (all purchases are after block 0)
        let trending = db.get_trending_content(10, 0).unwrap();
        assert_eq!(trending.len(), 1);
        assert_eq!(trending[0].0, "cid_001");
        assert_eq!(trending[0].1, 3); // 3 sales

        // Duplicate purchase (same tx_hash + content_id) should be ignored
        db.record_global_purchase("cid_001", "0xbuyer1", None, "1000000000000000", "0xtx1", 100, Some(1000), false).unwrap();
        let history2 = db.get_price_history("cid_001").unwrap();
        assert_eq!(history2.len(), 3); // still 3, not 4
    }

    #[test]
    fn test_top_collections_with_volume() {
        let db = Database::open_in_memory().unwrap();

        // Create collection and add content
        db.upsert_collection(1, "0xcreator", "Top Hits", "", "", 1000, true).unwrap();
        db.upsert_collection_item(1, "cid_001", 1001).unwrap();
        db.upsert_collection_item(1, "cid_002", 1002).unwrap();

        // Create content
        db.conn().execute(
            "INSERT INTO content (content_id, content_hash, creator, metadata_uri, price_wei, title, content_type, active, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 1000)",
            rusqlite::params!["cid_001", "h1", "0xcreator", "{}", "1000000000000000", "Song 1", "music"],
        ).unwrap();
        db.conn().execute(
            "INSERT INTO content (content_id, content_hash, creator, metadata_uri, price_wei, title, content_type, active, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 1000)",
            rusqlite::params!["cid_002", "h2", "0xcreator", "{}", "2000000000000000", "Song 2", "music"],
        ).unwrap();

        // Record purchases for collection items
        db.record_global_purchase("cid_001", "0xbuyer1", None, "1000000000000000", "0xtx1", 100, None, false).unwrap();
        db.record_global_purchase("cid_002", "0xbuyer2", None, "2000000000000000", "0xtx2", 101, None, false).unwrap();

        // Top collections should show volume
        let top = db.get_top_collections(10).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].1, "Top Hits"); // name
        assert_eq!(top[0].5, 2); // item_count = 2
        // volume_wei should be sum of purchases = 3000000000000000
        assert_eq!(top[0].6, "3000000000000000");
    }
}
