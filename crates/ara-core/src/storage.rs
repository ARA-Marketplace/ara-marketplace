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
            .execute("ALTER TABLE purchases ADD COLUMN downloaded_path TEXT", []);

        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
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
