use std::fs;
use std::path::Path;

use diesel::connection::{Connection, SimpleConnection};
use diesel::r2d2::{ConnectionManager, CustomizeConnection};
use diesel::SqliteConnection;
use hyper::client::{Client, HttpConnector};
use hyper_tls::HttpsConnector;

pub struct RmGuard<P: AsRef<Path>>(pub P);

#[derive(Debug)]
struct ConnectionCustomizer;

const DB_URL: &str = "websub.sqlite3";

impl<P: AsRef<Path>> Drop for RmGuard<P> {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for ConnectionCustomizer {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
        on_acquire(conn).map_err(diesel::r2d2::Error::QueryError)
    }
}

pub fn open_database() -> anyhow::Result<SqliteConnection> {
    let mut conn = SqliteConnection::establish(DB_URL)?;
    on_acquire(&mut conn)?;
    Ok(conn)
}

pub(crate) fn database_pool(
) -> anyhow::Result<crate::websub::Pool<ConnectionManager<SqliteConnection>>> {
    let ret = diesel::r2d2::Pool::builder()
        .connection_customizer(Box::new(ConnectionCustomizer))
        .build(ConnectionManager::new(DB_URL))?;
    Ok(ret.into())
}

pub fn http_client() -> Client<HttpsConnector<HttpConnector>> {
    Client::builder().build(HttpsConnector::new())
}

fn on_acquire(conn: &mut SqliteConnection) -> diesel::QueryResult<()> {
    // The value of `5000` ms is taken from `rusqlite`'s default.
    // <https://github.com/diesel-rs/diesel/issues/2365#issuecomment-719467312>
    // <https://github.com/rusqlite/rusqlite/commit/05b03ae2cec9f9f630095d5c0e89682da334f4a4>
    conn.batch_execute(
        "\
        PRAGMA busy_timeout=5000;\
        PRAGMA journal_mode=WAL;\
        ",
    )
}
