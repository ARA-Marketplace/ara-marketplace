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
                filename TEXT
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
                PRIMARY KEY (content_id, seeder_eth_address, buyer_eth_address)
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
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "INSERT OR IGNORE INTO delivery_receipts
             (content_id, seeder_eth_address, buyer_eth_address, signature, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![content_id, seeder_eth_address, buyer_eth_address, signature, timestamp],
        )
    }

    /// Get all delivery receipts for a content item.
    pub fn get_receipts_for_content(&self, content_id: &str) -> Result<Vec<DeliveryReceipt>> {
        let mut stmt = self.conn.prepare(
            "SELECT seeder_eth_address, buyer_eth_address, signature, timestamp
             FROM delivery_receipts WHERE content_id = ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![content_id], |row| {
            Ok(DeliveryReceipt {
                content_id: content_id.to_string(),
                seeder_eth_address: row.get(0)?,
                buyer_eth_address: row.get(1)?,
                signature: row.get(2)?,
                timestamp: row.get(3)?,
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

    /// Upsert a content row discovered from on-chain event sync.
    /// On conflict, updates price and metadata but preserves local publisher data.
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
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "INSERT INTO content
             (content_id, content_hash, creator, metadata_uri, price_wei,
              title, description, content_type, file_size_bytes, active,
              created_at, publisher_node_id, publisher_relay_url, filename)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11, ?12, ?13)
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
               active = 1",
            rusqlite::params![
                content_id, content_hash, creator, metadata_uri, price_wei,
                title, description, content_type, file_size, created_at,
                publisher_node_id, publisher_relay_url, filename,
            ],
        )
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
}
