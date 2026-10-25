use crate::{Database, Result};
use rusqlite::params;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
}
pub fn migrate(db: &Database) -> Result<()> {
    db.conn.execute_batch("CREATE TABLE IF NOT EXISTS catalog(id TEXT PRIMARY KEY,name TEXT NOT NULL,description TEXT NOT NULL,category TEXT NOT NULL,version TEXT NOT NULL); CREATE VIRTUAL TABLE IF NOT EXISTS catalog_fts USING fts5(id UNINDEXED,name,description,category);")?;
    Ok(())
}
pub fn upsert(db: &Database, e: &CatalogEntry, version: &str) -> Result<()> {
    migrate(db)?;
    db.conn.execute(
        "INSERT OR REPLACE INTO catalog VALUES(?1,?2,?3,?4,?5)",
        params![e.id, e.name, e.description, e.category, version],
    )?;
    db.conn.execute("INSERT OR REPLACE INTO catalog_fts(rowid,id,name,description,category) SELECT rowid,id,name,description,category FROM catalog WHERE id=?1", params![e.id])?;
    Ok(())
}
pub fn search(db: &Database, query: &str, limit: usize) -> Result<Vec<CatalogEntry>> {
    migrate(db)?;
    let mut s=db.conn.prepare("SELECT c.id,c.name,c.description,c.category FROM catalog c JOIN catalog_fts f ON f.id=c.id WHERE catalog_fts MATCH ?1 LIMIT ?2")?;
    let rows = s
        .query_map(params![query, limit as i64], |r| {
            Ok(CatalogEntry {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                category: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexes_and_finds() {
        let db = Database::in_memory().unwrap();
        upsert(
            &db,
            &CatalogEntry {
                id: "x".into(),
                name: "Waybar".into(),
                description: "bar theme".into(),
                category: "theme".into(),
            },
            "1",
        )
        .unwrap();
        assert_eq!(search(&db, "Waybar", 10).unwrap().len(), 1);
    }
}
