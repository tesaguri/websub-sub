use std::fs;
use std::path::Path;

use diesel::dsl::sql_query;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, CustomizeConnection, Pool};
use diesel::{Connection, SqliteConnection};
use hyper::client::{Client, HttpConnector};
use hyper_tls::HttpsConnector;

#[derive(Debug)]
struct ConnectionCustomizer;

pub struct RmOnDrop<P: AsRef<Path>>(pub P);

const DB_URL: &str = "websub.sqlite3";

impl<P: AsRef<Path>> Drop for RmOnDrop<P> {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for ConnectionCustomizer {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
        pragma_foreign_keys_on(conn).map_err(diesel::r2d2::Error::QueryError)
    }
}

pub fn open_database() -> SqliteConnection {
    let conn = SqliteConnection::establish(DB_URL).unwrap();
    pragma_foreign_keys_on(&conn).unwrap();
    conn
}

pub fn database_pool() -> Pool<ConnectionManager<SqliteConnection>> {
    let manager = ConnectionManager::<SqliteConnection>::new(DB_URL);
    Pool::builder()
        .connection_customizer(Box::new(ConnectionCustomizer))
        .build(manager)
        .unwrap()
}

pub fn http_client() -> Client<HttpsConnector<HttpConnector>> {
    Client::builder().build(HttpsConnector::new())
}

fn pragma_foreign_keys_on(conn: &SqliteConnection) -> Result<(), diesel::result::Error> {
    sql_query("PRAGMA foreign_keys = ON")
        .execute(conn)
        .map(|_| ())
}
