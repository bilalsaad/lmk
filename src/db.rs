// Simple KV store.

use rusqlite::{Connection, Result};

pub struct Db {
    connection: Connection,
}

impl Db {
    // Creates w/ the given sqllite3 file.
    pub fn new(db_path: &str) -> Result<Self> {
        let connection = Connection::open(db_path)?;
        connection.execute(
            "create table if not exists kv (key text unique, value text)",
            (),
        )?;
        Ok(Db { connection })
    }
    #[cfg(test)]
    pub fn new_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        connection.execute(
            "create table if not exists kv (key text unique, value text)",
            (),
        )?;
        Ok(Db { connection })
    }
    pub fn get(&self, key: &str) -> Option<String> {
        let mut stmt = match self
            .connection
            .prepare("SELECT value FROM kv WHERE key = ?")
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("failed to read from db... {}", e);
                return None;
            }
        };
        // not sure why we needed row.get(0) here..
        stmt.query_row(rusqlite::params![key], |row| row.get(0))
            .ok()
    }

    pub fn put(&mut self, key: &str, value: &str) -> Result<()> {
        self.connection
            .execute("REPLACE INTO kv (key, value) VALUES (?1, ?2)", (key, value))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple() -> Result<(), Box<dyn std::error::Error>> {
        let mut db = Db::new_in_memory()?;
        assert_eq!(db.get("a"), None);
        db.put("a", "b")?;
        assert_eq!(db.get("b"), None);
        assert_eq!(db.get("a"), Some("b".to_string()));
        Ok(())
    }

    #[test]
    fn test_override() -> Result<(), Box<dyn std::error::Error>> {
        let mut db = Db::new_in_memory()?;
        db.put("a", "b")?;
        assert_eq!(db.get("a"), Some("b".to_string()));
        db.put("a", "q")?;
        assert_eq!(db.get("a"), Some("q".to_string()));
        Ok(())
    }
}
