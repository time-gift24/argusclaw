#[cfg(test)]
mod tests {
    use crate::sqlite::{ArgusSqlite, DbResult, connect_path, migrate};
    use crate::traits::SessionRepository;
    use tempfile::TempDir;

    async fn create_test_db() -> DbResult<(TempDir, ArgusSqlite)> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = connect_path(&path).await?;
        migrate(&pool).await?;
        Ok((dir, ArgusSqlite::new(pool)))
    }

    #[tokio::test]
    async fn create_and_get_session() -> DbResult<()> {
        let (_dir, db) = create_test_db().await?;

        let id = db.create_session("test-session").await?;
        let session = db.get_session(&id).await?;

        assert!(session.is_some());
        assert_eq!(session.unwrap().name, "test-session");
        Ok(())
    }

    #[tokio::test]
    async fn list_sessions_empty() -> DbResult<()> {
        let (_dir, db) = create_test_db().await?;

        let sessions = db.list_sessions().await?;
        // Migration creates a default "Legacy" session
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "Legacy");
        Ok(())
    }

    #[tokio::test]
    async fn update_session() -> DbResult<()> {
        let (_dir, db) = create_test_db().await?;

        let id = db.create_session("original").await?;
        db.update_session(&id, "renamed").await?;

        let session = db.get_session(&id).await?.unwrap();
        assert_eq!(session.name, "renamed");
        Ok(())
    }

    #[tokio::test]
    async fn delete_session() -> DbResult<()> {
        let (_dir, db) = create_test_db().await?;

        let id = db.create_session("to-delete").await?;
        let deleted = db.delete_session(&id).await?;
        assert!(deleted);

        let session = db.get_session(&id).await?;
        assert!(session.is_none());
        Ok(())
    }
}
