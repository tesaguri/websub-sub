use std::any::type_name;
use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;
use std::mem;

use diesel2::associations::HasTable;
use diesel2::backend::sql_dialect::exists_syntax::AnsiSqlExistsSyntax;
use diesel2::backend::sql_dialect::from_clause_syntax::AnsiSqlFromClauseSyntax;
use diesel2::backend::sql_dialect::select_statement_syntax::AnsiSqlSelectStatement;
use diesel2::backend::{Backend, DieselReserveSpecialization, SqlDialect};
use diesel2::connection::{DefaultLoadingMode, LoadConnection};
use diesel2::deserialize::FromSql;
use diesel2::dsl::*;
use diesel2::expression::{is_aggregate, AsExpression, ValidGrouping};
use diesel2::prelude::*;
use diesel2::query_builder::{
    AsQuery, DefaultValues, FromClause, LimitOffsetClause, NoLimitClause, NoOffsetClause,
    QueryFragment, QueryId, SelectStatement,
};
use diesel2::r2d2::{self, ManageConnection, PooledConnection};
use diesel2::serialize::ToSql;
use diesel2::sql_types::{self, HasSqlType};
use rand::RngCore;

use super::ConnectionRef;

#[repr(transparent)]
pub struct Connection<C, Id, Hub, Topic, Secret, ExpiresAt> {
    inner: C,
    #[allow(clippy::type_complexity)]
    marker: PhantomData<fn() -> (Id, Hub, Topic, Secret, ExpiresAt)>,
}

pub struct Pool<M: r2d2::ManageConnection, Id, Hub, Topic, Secret, ExpiresAt> {
    inner: r2d2::Pool<M>,
    #[allow(clippy::type_complexity)]
    marker: PhantomData<fn() -> (Id, Hub, Topic, Secret, ExpiresAt)>,
}

/// Type alias used to refer to the private `diesel2::expression::bound::Bound` type.
type Bound<ST, T> = <T as AsExpression<ST>>::Expression;

impl<C, DB, Table, Id, Hub, Topic, Secret, ExpiresAt>
    Connection<C, Id, Hub, Topic, Secret, ExpiresAt>
where
    C: diesel2::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
    DB: Backend
        + SqlDialect<
            EmptyFromClauseSyntax = AnsiSqlFromClauseSyntax,
            SelectStatementSyntax = AnsiSqlSelectStatement,
            ExistsSyntax = AnsiSqlExistsSyntax,
        > + DieselReserveSpecialization
        + HasSqlType<sql_types::BigInt>
        + HasSqlType<sql_types::Bool>
        + HasSqlType<sql_types::Text>
        + 'static,
    DefaultValues: QueryFragment<DB>,
    LimitOffsetClause<NoLimitClause, NoOffsetClause>: QueryFragment<DB>,
    bool: FromSql<sql_types::Bool, DB>,
    i64: FromSql<sql_types::BigInt, DB> + ToSql<sql_types::BigInt, DB>,
    for<'a> &'a str: ToSql<sql_types::Text, DB>,
    String: FromSql<sql_types::Text, DB>,
    Table: diesel2::Table<PrimaryKey = Id, AllColumns = (Id, Hub, Topic, Secret, ExpiresAt)>
        + HasTable<Table = Table>
        + AsQuery<Query = SelectStatement<FromClause<Table>>>
        + QuerySource<DefaultSelection = <Table as diesel2::Table>::AllColumns>
        + QueryId
        + Default
        + 'static,
    Table::FromClause: QueryFragment<DB>,
    Id: Column<Table = Table>
        + Expression<SqlType = sql_types::BigInt>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + EqAll<i64, Output = Eq<Id, Bound<sql_types::BigInt, i64>>>
        + Default,
    Hub: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
    Topic: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
    Secret: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
    ExpiresAt: Column<Table = Table>
        + Expression<SqlType = sql_types::Nullable<sql_types::BigInt>>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
{
    pub fn new(connection: C) -> Self {
        Connection {
            inner: connection,
            marker: PhantomData,
        }
    }
}

impl<C, Id, Hub, Topic, Secret, ExpiresAt> Connection<C, Id, Hub, Topic, Secret, ExpiresAt> {
    pub fn into_inner(self) -> C {
        self.inner
    }
}

impl<C, DB, Table, Id, Hub, Topic, Secret, ExpiresAt> super::Connection
    for Connection<C, Id, Hub, Topic, Secret, ExpiresAt>
where
    C: diesel2::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
    DB: Backend
        + SqlDialect<
            EmptyFromClauseSyntax = AnsiSqlFromClauseSyntax,
            SelectStatementSyntax = AnsiSqlSelectStatement,
            ExistsSyntax = AnsiSqlExistsSyntax,
        > + DieselReserveSpecialization
        + HasSqlType<sql_types::BigInt>
        + HasSqlType<sql_types::Bool>
        + HasSqlType<sql_types::Text>
        + 'static,
    DefaultValues: QueryFragment<DB>,
    LimitOffsetClause<NoLimitClause, NoOffsetClause>: QueryFragment<DB>,
    bool: FromSql<sql_types::Bool, DB>,
    i64: FromSql<sql_types::BigInt, DB> + ToSql<sql_types::BigInt, DB>,
    for<'a> &'a str: ToSql<sql_types::Text, DB>,
    String: FromSql<sql_types::Text, DB>,
    Table: diesel2::Table<PrimaryKey = Id, AllColumns = (Id, Hub, Topic, Secret, ExpiresAt)>
        + HasTable<Table = Table>
        + AsQuery<Query = SelectStatement<FromClause<Table>>>
        + QuerySource<DefaultSelection = <Table as diesel2::Table>::AllColumns>
        + QueryId
        + Default
        + 'static,
    Table::FromClause: QueryFragment<DB>,
    Id: Column<Table = Table>
        + Expression<SqlType = sql_types::BigInt>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + EqAll<i64, Output = Eq<Id, Bound<sql_types::BigInt, i64>>>
        + Default,
    Hub: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
    Topic: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
    Secret: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
    ExpiresAt: Column<Table = Table>
        + Expression<SqlType = sql_types::Nullable<sql_types::BigInt>>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
{
    type Ref<'a> = &'a mut Self
    where
        Self: 'a;
    type Error = diesel2::result::Error;

    fn as_conn_ref(&mut self) -> Self::Ref<'_> {
        self
    }
}

impl<C, DB, Table, Id, Hub, Topic, Secret, ExpiresAt> ConnectionRef
    for &mut Connection<C, Id, Hub, Topic, Secret, ExpiresAt>
where
    C: diesel2::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
    DB: Backend
        + SqlDialect<
            EmptyFromClauseSyntax = AnsiSqlFromClauseSyntax,
            SelectStatementSyntax = AnsiSqlSelectStatement,
            ExistsSyntax = AnsiSqlExistsSyntax,
        > + DieselReserveSpecialization
        + HasSqlType<sql_types::BigInt>
        + HasSqlType<sql_types::Bool>
        + HasSqlType<sql_types::Text>
        + 'static,
    DefaultValues: QueryFragment<DB>,
    LimitOffsetClause<NoLimitClause, NoOffsetClause>: QueryFragment<DB>,
    bool: FromSql<sql_types::Bool, DB>,
    i64: FromSql<sql_types::BigInt, DB> + ToSql<sql_types::BigInt, DB>,
    for<'a> &'a str: ToSql<sql_types::Text, DB>,
    String: FromSql<sql_types::Text, DB>,
    Table: diesel2::Table<PrimaryKey = Id, AllColumns = (Id, Hub, Topic, Secret, ExpiresAt)>
        + HasTable<Table = Table>
        + AsQuery<Query = SelectStatement<FromClause<Table>>>
        + QuerySource<DefaultSelection = <Table as diesel2::Table>::AllColumns>
        + QueryId
        + Default
        + 'static,
    Table::FromClause: QueryFragment<DB>,
    Id: Column<Table = Table>
        + Expression<SqlType = sql_types::BigInt>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + EqAll<i64, Output = Eq<Id, Bound<sql_types::BigInt, i64>>>
        + Default,
    Hub: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
    Topic: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
    Secret: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
    ExpiresAt: Column<Table = Table>
        + Expression<SqlType = sql_types::Nullable<sql_types::BigInt>>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default,
{
    type Reborrowed<'a> = &'a mut Connection<C, Id, Hub, Topic, Secret, ExpiresAt>
    where
        Self: 'a;
    type Error = diesel2::result::Error;

    fn reborrow(&mut self) -> Self::Reborrowed<'_> {
        self
    }

    fn transaction<T, F>(&mut self, f: F) -> Result<T, Self::Error>
    where
        F: FnOnce(&mut Self) -> Result<T, Self::Error>,
    {
        self.inner.transaction(|inner| {
            let mut this = unsafe {
                // SAFETY:
                // The `#[repr(transparent)]` attribute on `Connection` declaration
                // guarantees that `Connection<..>` has the same layout as `C`, ensuring
                // the soundness of the transmution.
                mem::transmute::<&mut C, &mut Connection<C, Id, Hub, Topic, Secret, ExpiresAt>>(
                    inner,
                )
            };
            f(&mut this)
        })
    }

    fn create_subscription(&mut self, hub: &str, topic: &str, secret: &str) -> QueryResult<u64> {
        let mut rng = rand::thread_rng();
        self.transaction(|this| loop {
            let id = rng.next_u64();
            let result = diesel2::insert_into(Table::default())
                .values((
                    Id::default().eq(id as i64),
                    Hub::default().eq(hub),
                    Topic::default().eq(topic),
                    Secret::default().eq(secret),
                ))
                .execute(&mut this.inner);
            match result {
                Ok(_) => return Ok(id),
                Err(diesel2::result::Error::DatabaseError(
                    diesel2::result::DatabaseErrorKind::UniqueViolation,
                    _,
                )) => {} // retry
                Err(e) => return Err(e),
            }
        })
    }

    fn get_topic(&mut self, id: u64) -> QueryResult<Option<(String, String)>> {
        Table::default()
            .select((Topic::default(), Secret::default()))
            .find(id as i64)
            .get_result::<(String, String)>(&mut self.inner)
            .optional()
    }

    fn subscription_exists(&mut self, id: u64, topic: &str) -> QueryResult<bool> {
        diesel2::select(exists(
            Table::default()
                .find(id as i64)
                .filter(Topic::default().eq(topic)),
        ))
        .get_result(&mut self.inner)
    }

    fn get_subscriptions_expire_before(
        &mut self,
        before: i64,
    ) -> QueryResult<Vec<(u64, String, String)>> {
        Table::default()
            .filter(ExpiresAt::default().le(before))
            .select((Id::default(), Hub::default(), Topic::default()))
            .load_iter::<(i64, String, String), _>(&mut self.inner)?
            .map(|result| result.map(|(id, hub, topic)| (id as u64, hub, topic)))
            .collect()
    }

    fn get_hub_of_inactive_subscription(
        &mut self,
        id: u64,
        topic: &str,
    ) -> QueryResult<Option<String>> {
        Table::default()
            .find(id as i64)
            .filter(Topic::default().eq(topic))
            .filter(ExpiresAt::default().is_null())
            .select(Hub::default())
            .get_result(&mut self.inner)
            .optional()
    }

    fn get_old_subscriptions(&mut self, id: u64, hub: &str, topic: &str) -> QueryResult<Vec<u64>> {
        Table::default()
            .filter(Hub::default().eq(&hub))
            .filter(Topic::default().eq(&topic))
            .filter(not(Id::default().eq(id as i64)))
            .select(Id::default())
            .load_iter(&mut self.inner)?
            .map(|result| result.map(|id: i64| id as u64))
            .collect()
    }

    fn activate_subscription(&mut self, id: u64, expires_at: i64) -> QueryResult<bool> {
        diesel2::update(Table::default().find(id as i64))
            .set(ExpiresAt::default().eq(expires_at))
            .execute(&mut self.inner)
            .map(|n| n != 0)
    }

    fn deactivate_subscriptions_expire_before(&mut self, before: i64) -> QueryResult<()> {
        diesel2::update(Table::default().filter(ExpiresAt::default().le(before)))
            .set(ExpiresAt::default().eq(None::<i64>))
            .execute(&mut self.inner)
            .map(|_| ())
    }

    fn delete_subscriptions(&mut self, id: u64) -> QueryResult<bool> {
        diesel2::delete(Table::default().find(id as i64))
            .execute(&mut self.inner)
            .map(|n| n != 0)
    }

    fn get_next_expiry(&mut self) -> QueryResult<Option<i64>> {
        Table::default()
            .filter(ExpiresAt::default().is_not_null())
            .select(ExpiresAt::default().assume_not_null())
            .order(ExpiresAt::default().asc())
            // XXX: Not using `first()`, which involves a `LIMIT` clause that introduces
            // a private type `diesel2::expression::Bound` to the trait bound puzzle
            // (`LimitOffsetClause<LimitClause<Bound<BigInt, i64>>, NoOffsetClause>: QueryFragment<DB>`).
            // The `self::Bound` type alias won't work somehow.
            .load_iter(&mut self.inner)?
            .next()
            .transpose()
    }
}

impl<C, Id, Hub, Topic, Secret, ExpiresAt> AsRef<C>
    for Connection<C, Id, Hub, Topic, Secret, ExpiresAt>
{
    fn as_ref(&self) -> &C {
        &self.inner
    }
}

impl<C, Id, Hub, Topic, Secret, ExpiresAt> AsMut<C>
    for Connection<C, Id, Hub, Topic, Secret, ExpiresAt>
{
    fn as_mut(&mut self) -> &mut C {
        &mut self.inner
    }
}

impl<C: Debug, Id, Hub, Topic, Secret, ExpiresAt> Debug
    for Connection<C, Id, Hub, Topic, Secret, ExpiresAt>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Connection<_, {}, {}, {}, {}, {}",
            type_name::<Id>(),
            type_name::<Hub>(),
            type_name::<Topic>(),
            type_name::<Secret>(),
            type_name::<ExpiresAt>()
        )?;
        // XXX: `debug_tuple` only accepts a `&str` so it cannot be like
        // `debug_tuple(format_args!("Connection<..>", ..))`.
        f.debug_tuple(">").field(&self.inner).finish()
    }
}

impl<M: ManageConnection, DB, Table, Id, Hub, Topic, Secret, ExpiresAt>
    Pool<M, Id, Hub, Topic, Secret, ExpiresAt>
where
    PooledConnection<M>: diesel2::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
    DB: Backend
        + SqlDialect<
            EmptyFromClauseSyntax = AnsiSqlFromClauseSyntax,
            SelectStatementSyntax = AnsiSqlSelectStatement,
            ExistsSyntax = AnsiSqlExistsSyntax,
        > + DieselReserveSpecialization
        + HasSqlType<sql_types::BigInt>
        + HasSqlType<sql_types::Bool>
        + HasSqlType<sql_types::Text>
        + 'static,
    DefaultValues: QueryFragment<DB>,
    LimitOffsetClause<NoLimitClause, NoOffsetClause>: QueryFragment<DB>,
    bool: FromSql<sql_types::Bool, DB>,
    i64: FromSql<sql_types::BigInt, DB> + ToSql<sql_types::BigInt, DB>,
    for<'a> &'a str: ToSql<sql_types::Text, DB>,
    String: FromSql<sql_types::Text, DB>,
    Table: diesel2::Table<PrimaryKey = Id, AllColumns = (Id, Hub, Topic, Secret, ExpiresAt)>
        + HasTable<Table = Table>
        + AsQuery<Query = SelectStatement<FromClause<Table>>>
        + QuerySource<DefaultSelection = <Table as diesel2::Table>::AllColumns>
        + QueryId
        + Default
        + 'static,
    Table::FromClause: QueryFragment<DB>,
    Id: Column<Table = Table>
        + Expression<SqlType = sql_types::BigInt>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + EqAll<i64, Output = Eq<Id, Bound<sql_types::BigInt, i64>>>
        + Default
        + 'static,
    Hub: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
    Topic: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
    Secret: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
    ExpiresAt: Column<Table = Table>
        + Expression<SqlType = sql_types::Nullable<sql_types::BigInt>>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
{
    pub fn new(manager: M) -> Result<Self, r2d2::PoolError> {
        r2d2::Pool::new(manager).map(Pool::from)
    }
}

impl<M: ManageConnection, Id, Hub, Topic, Secret, ExpiresAt>
    Pool<M, Id, Hub, Topic, Secret, ExpiresAt>
{
    pub fn into_inner(self) -> r2d2::Pool<M> {
        self.inner
    }
}

impl<M: ManageConnection, Id, Hub, Topic, Secret, ExpiresAt> AsRef<r2d2::Pool<M>>
    for Pool<M, Id, Hub, Topic, Secret, ExpiresAt>
{
    fn as_ref(&self) -> &r2d2::Pool<M> {
        &self.inner
    }
}

impl<M: ManageConnection, DB, Table, Id, Hub, Topic, Secret, ExpiresAt> From<r2d2::Pool<M>>
    for Pool<M, Id, Hub, Topic, Secret, ExpiresAt>
where
    PooledConnection<M>: diesel2::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
    DB: Backend
        + SqlDialect<
            EmptyFromClauseSyntax = AnsiSqlFromClauseSyntax,
            SelectStatementSyntax = AnsiSqlSelectStatement,
            ExistsSyntax = AnsiSqlExistsSyntax,
        > + DieselReserveSpecialization
        + HasSqlType<sql_types::BigInt>
        + HasSqlType<sql_types::Bool>
        + HasSqlType<sql_types::Text>
        + 'static,
    DefaultValues: QueryFragment<DB>,
    LimitOffsetClause<NoLimitClause, NoOffsetClause>: QueryFragment<DB>,
    bool: FromSql<sql_types::Bool, DB>,
    i64: FromSql<sql_types::BigInt, DB> + ToSql<sql_types::BigInt, DB>,
    for<'a> &'a str: ToSql<sql_types::Text, DB>,
    String: FromSql<sql_types::Text, DB>,
    Table: diesel2::Table<PrimaryKey = Id, AllColumns = (Id, Hub, Topic, Secret, ExpiresAt)>
        + HasTable<Table = Table>
        + AsQuery<Query = SelectStatement<FromClause<Table>>>
        + QuerySource<DefaultSelection = <Table as diesel2::Table>::AllColumns>
        + QueryId
        + Default
        + 'static,
    Table::FromClause: QueryFragment<DB>,
    Id: Column<Table = Table>
        + Expression<SqlType = sql_types::BigInt>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + EqAll<i64, Output = Eq<Id, Bound<sql_types::BigInt, i64>>>
        + Default
        + 'static,
    Hub: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
    Topic: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
    Secret: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
    ExpiresAt: Column<Table = Table>
        + Expression<SqlType = sql_types::Nullable<sql_types::BigInt>>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
{
    fn from(pool: r2d2::Pool<M>) -> Self {
        Pool {
            inner: pool,
            marker: PhantomData,
        }
    }
}

impl<M: ManageConnection, Id, Hub, Topic, Secret, ExpiresAt> Clone
    for Pool<M, Id, Hub, Topic, Secret, ExpiresAt>
{
    fn clone(&self) -> Self {
        Pool {
            inner: self.inner.clone(),
            marker: PhantomData,
        }
    }
}

impl<M: ManageConnection, Id, Hub, Topic, Secret, ExpiresAt> Debug
    for Pool<M, Id, Hub, Topic, Secret, ExpiresAt>
where
    r2d2::Pool<M>: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Pool<_, {}, {}, {}, {}, {}",
            type_name::<Id>(),
            type_name::<Hub>(),
            type_name::<Topic>(),
            type_name::<Secret>(),
            type_name::<ExpiresAt>()
        )?;
        f.debug_tuple(">").field(&self.inner).finish()
    }
}

impl<M: ManageConnection, DB, Table, Id, Hub, Topic, Secret, ExpiresAt> super::Pool
    for Pool<M, Id, Hub, Topic, Secret, ExpiresAt>
where
    PooledConnection<M>: diesel2::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
    DB: Backend
        + SqlDialect<
            EmptyFromClauseSyntax = AnsiSqlFromClauseSyntax,
            SelectStatementSyntax = AnsiSqlSelectStatement,
            ExistsSyntax = AnsiSqlExistsSyntax,
        > + DieselReserveSpecialization
        + HasSqlType<sql_types::BigInt>
        + HasSqlType<sql_types::Bool>
        + HasSqlType<sql_types::Text>
        + 'static,
    DefaultValues: QueryFragment<DB>,
    LimitOffsetClause<NoLimitClause, NoOffsetClause>: QueryFragment<DB>,
    bool: FromSql<sql_types::Bool, DB>,
    i64: FromSql<sql_types::BigInt, DB> + ToSql<sql_types::BigInt, DB>,
    for<'a> &'a str: ToSql<sql_types::Text, DB>,
    String: FromSql<sql_types::Text, DB>,
    Table: diesel2::Table<PrimaryKey = Id, AllColumns = (Id, Hub, Topic, Secret, ExpiresAt)>
        + HasTable<Table = Table>
        + AsQuery<Query = SelectStatement<FromClause<Table>>>
        + QuerySource<DefaultSelection = <Table as diesel2::Table>::AllColumns>
        + QueryId
        + Default
        + 'static,
    Table::FromClause: QueryFragment<DB>,
    Id: Column<Table = Table>
        + Expression<SqlType = sql_types::BigInt>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + EqAll<i64, Output = Eq<Id, Bound<sql_types::BigInt, i64>>>
        + Default
        + 'static,
    Hub: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
    Topic: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
    Secret: Column<Table = Table>
        + Expression<SqlType = sql_types::Text>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
    ExpiresAt: Column<Table = Table>
        + Expression<SqlType = sql_types::Nullable<sql_types::BigInt>>
        + SelectableExpression<Table>
        + QueryFragment<DB>
        + QueryId
        + ValidGrouping<(), IsAggregate = is_aggregate::No>
        + Default
        + 'static,
{
    type Connection = Connection<PooledConnection<M>, Id, Hub, Topic, Secret, ExpiresAt>;
    type Error = r2d2::PoolError;

    fn get(&self) -> Result<Self::Connection, Self::Error> {
        self.inner.get().map(Connection::new)
    }
}
