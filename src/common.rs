use diesel::r2d2::{ConnectionManager, Pool};
use diesel::{Connection, SqliteConnection};

const DB_URL: &str = "websub.sqlite3";

pub fn open_database() -> SqliteConnection {
    SqliteConnection::establish(DB_URL).unwrap()
}

pub fn database_pool() -> Pool<ConnectionManager<SqliteConnection>> {
    let manager = ConnectionManager::<SqliteConnection>::new(DB_URL);
    Pool::new(manager).unwrap()
}
