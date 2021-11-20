use std::fs;
use std::path::Path;

use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::{Connection, SqliteConnection};
use hyper::client::{Client, HttpConnector};
use hyper_tls::HttpsConnector;

use crate::query;

pub struct RmGuard<P: AsRef<Path>>(pub P);

const DB_URL: &str = "websub.sqlite3";

impl<P: AsRef<Path>> Drop for RmGuard<P> {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn open_database() -> anyhow::Result<SqliteConnection> {
    let conn = SqliteConnection::establish(DB_URL)?;
    query::pragma_busy_timeout(5000).execute(&conn)?;
    query::pragma_foreign_keys_on().execute(&conn)?;
    query::pragma_journal_mode_wal().execute(&conn)?;
    Ok(conn)
}

pub fn database_pool() -> anyhow::Result<Pool<ConnectionManager<SqliteConnection>>> {
    Ok(crate::util::r2d2::new_pool(ConnectionManager::new(DB_URL))?)
}

pub fn http_client() -> Client<HttpsConnector<HttpConnector>> {
    Client::builder().build(HttpsConnector::new())
}
