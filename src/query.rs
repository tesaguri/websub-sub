use diesel::dsl::*;
use diesel::prelude::*;
use diesel::query_builder::{AstPass, QueryFragment};
use diesel::sqlite::Sqlite;

use crate::schema::*;

#[derive(QueryId)]
pub struct PragmaBusyTimeout {
    milliseconds: u32,
}

#[derive(QueryId)]
pub struct PragmaForeignKeysOn {
    _priv: (),
}

#[derive(QueryId)]
pub struct PragmaJournalModeWal {
    _priv: (),
}

impl QueryFragment<Sqlite> for PragmaBusyTimeout {
    fn walk_ast(&self, mut out: AstPass<'_, Sqlite>) -> QueryResult<()> {
        out.push_sql("PRAGMA busy_timeout=");
        // Bind variables seems not to be unusable here.
        let mut buf = itoa::Buffer::new();
        out.push_sql(buf.format(self.milliseconds));
        Ok(())
    }
}

impl RunQueryDsl<SqliteConnection> for PragmaBusyTimeout {}

impl QueryFragment<Sqlite> for PragmaForeignKeysOn {
    fn walk_ast(&self, mut out: AstPass<'_, Sqlite>) -> QueryResult<()> {
        out.push_sql("PRAGMA foreign_keys=ON");
        Ok(())
    }
}

impl RunQueryDsl<SqliteConnection> for PragmaForeignKeysOn {}

impl QueryFragment<Sqlite> for PragmaJournalModeWal {
    fn walk_ast(&self, mut out: AstPass<'_, Sqlite>) -> QueryResult<()> {
        out.push_sql("PRAGMA journal_mode=WAL");
        Ok(())
    }
}

impl RunQueryDsl<SqliteConnection> for PragmaJournalModeWal {}

pub fn expires_at() -> Order<
    Select<active_subscriptions::table, active_subscriptions::expires_at>,
    Asc<active_subscriptions::expires_at>,
> {
    active_subscriptions::table
        .select(active_subscriptions::expires_at)
        .order(active_subscriptions::expires_at.asc())
}

pub fn pragma_busy_timeout(milliseconds: u32) -> PragmaBusyTimeout {
    PragmaBusyTimeout { milliseconds }
}

pub fn pragma_foreign_keys_on() -> PragmaForeignKeysOn {
    PragmaForeignKeysOn { _priv: () }
}

pub fn pragma_journal_mode_wal() -> PragmaJournalModeWal {
    PragmaJournalModeWal { _priv: () }
}

pub fn renewing_subs() -> Select<renewing_subscriptions::table, renewing_subscriptions::old> {
    renewing_subscriptions::table.select(renewing_subscriptions::old)
}
