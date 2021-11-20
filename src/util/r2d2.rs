use diesel::prelude::*;
use diesel::r2d2::CustomizeConnection;
#[cfg(test)]
use diesel::r2d2::{self, ConnectionManager, Pool};

#[derive(Debug)]
struct ConnectionCustomizer;

impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for ConnectionCustomizer {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
        diesel::sql_query("PRAGMA foreign_keys = ON;")
            .execute(conn)
            .map_err(diesel::r2d2::Error::QueryError)?;
        Ok(())
    }
}

#[cfg(test)]
pub fn pool_with_builder(
    builder: r2d2::Builder<ConnectionManager<SqliteConnection>>,
    manager: ConnectionManager<SqliteConnection>,
) -> Result<Pool<ConnectionManager<SqliteConnection>>, diesel::r2d2::PoolError> {
    builder
        .connection_customizer(Box::new(ConnectionCustomizer))
        .build(manager)
}
