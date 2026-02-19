use anyhow::Result;
use rusqlite::Connection;

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
