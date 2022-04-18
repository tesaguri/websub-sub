use std::mem;

use diesel::backend::Backend;
use diesel::deserialize::FromSql;
use diesel::dsl::*;
use diesel::expression::bound::Bound;
use diesel::insertable::{ColumnInsertValue, InsertValues};
use diesel::prelude::*;
use diesel::r2d2::{ManageConnection, PooledConnection};
use diesel::sql_types::{self, HasSqlType};
use rand::RngCore;

use crate::schema::*;

pub struct Connection<C> {
    inner: C,
}

pub struct Pool<M>
where
    M: ManageConnection,
{
    inner: diesel::r2d2::Pool<M>,
}

impl<C: diesel::Connection> Connection<C>
where
    C::Backend: Backend + HasSqlType<sql_types::Bool> + 'static,
    i64: FromSql<sql_types::BigInt, C::Backend>,
    bool: FromSql<sql_types::Bool, C::Backend>,
    *const str: FromSql<sql_types::Text, C::Backend>,
    // XXX: Can we remove these `#[doc(hidden)]` types? These bounds are required for
    // `insert_into(subscriptions::table).values((id.eq(_), hub.eq(_), topic.eq(_), secret.eq(_)))`
    // to implement `ExecuteDsl<C>`. These traits are implemented differently between SQLite and
    // other backends and there seems to be no concise way to be generic over them.
    ColumnInsertValue<subscriptions::id, Bound<sql_types::BigInt, i64>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::hub, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::topic, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::secret, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
{
    pub fn new(connection: C) -> Self {
        Connection { inner: connection }
    }
}

impl<C: diesel::Connection> super::Connection for Connection<C>
where
    C::Backend: Backend + HasSqlType<sql_types::Bool> + 'static,
    i64: FromSql<sql_types::BigInt, C::Backend>,
    bool: FromSql<sql_types::Bool, C::Backend>,
    *const str: FromSql<sql_types::Text, C::Backend>,
    // XXX: Can we remove these `#[doc(hidden)]` types? These bounds are required for
    // `insert_into(subscriptions::table).values((id.eq(_), hub.eq(_), topic.eq(_), secret.eq(_)))`
    // to implement `ExecuteDsl<C>`. These traits are implemented differently between SQLite and
    // other backends and there seems to be no concise way to be generic over them.
    ColumnInsertValue<subscriptions::id, Bound<sql_types::BigInt, i64>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::hub, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::topic, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::secret, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
{
    type Error = diesel::result::Error;

    fn transaction<T, F>(&self, f: F) -> Result<T, Self::Error>
    where
        F: FnOnce() -> Result<T, Self::Error>,
    {
        self.inner.transaction(f)
    }

    fn create_subscription(&self, hub: &str, topic: &str, secret: &str) -> QueryResult<u64> {
        let mut rng = rand::thread_rng();
        self.transaction(|| loop {
            let id = rng.next_u64();
            let result = diesel::insert_into(subscriptions::table)
                .values((
                    subscriptions::id.eq(id as i64),
                    subscriptions::hub.eq(hub),
                    subscriptions::topic.eq(topic),
                    subscriptions::secret.eq(secret),
                ))
                .execute(&self.inner);
            match result {
                Ok(_) => return Ok(id),
                Err(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    _,
                )) => {} // retry
                Err(e) => return Err(e),
            }
        })
    }

    fn get_topic(&self, id: u64) -> QueryResult<Option<(String, String)>> {
        subscriptions::table
            .select((subscriptions::topic, subscriptions::secret))
            .find(id as i64)
            .get_result::<(String, String)>(&self.inner)
            .optional()
    }

    fn subscription_exists(&self, id: u64, topic: &str) -> QueryResult<bool> {
        diesel::select(exists(
            subscriptions::table
                .find(id as i64)
                .filter(subscriptions::topic.eq(topic)),
        ))
        .get_result(&self.inner)
    }

    fn get_subscriptions_expire_before(
        &self,
        before: i64,
    ) -> QueryResult<Vec<(i64, String, String)>> {
        subscriptions::table
            .filter(subscriptions::expires_at.le(before))
            .select((subscriptions::id, subscriptions::hub, subscriptions::topic))
            .load::<(i64, String, String)>(&self.inner)
    }

    fn get_hub_of_inactive_subscription(
        &self,
        id: u64,
        topic: &str,
    ) -> QueryResult<Option<String>> {
        subscriptions::table
            .find(id as i64)
            .filter(subscriptions::topic.eq(topic))
            .filter(subscriptions::expires_at.is_null())
            .select(subscriptions::hub)
            .get_result(&self.inner)
            .optional()
    }

    fn get_old_subscriptions(&self, id: u64, hub: &str, topic: &str) -> QueryResult<Vec<u64>> {
        let vec: Vec<i64> = subscriptions::table
            .filter(subscriptions::hub.eq(&hub))
            .filter(subscriptions::topic.eq(&topic))
            .filter(not(subscriptions::id.eq(id as i64)))
            .select(subscriptions::id)
            .load(&self.inner)?;
        let mut vec = mem::ManuallyDrop::new(vec);
        let (ptr, length, capacity) = (vec.as_mut_ptr(), vec.len(), vec.capacity());
        // Safety: `u64` has the same size and alignment as `i64`.
        let vec = unsafe { Vec::from_raw_parts(ptr.cast::<u64>(), length, capacity) };
        Ok(vec)
    }

    fn activate_subscription(&self, id: u64, expires_at: i64) -> QueryResult<bool> {
        diesel::update(subscriptions::table.find(id as i64))
            .set(subscriptions::expires_at.eq(expires_at))
            .execute(&self.inner)
            .map(|n| n != 0)
    }

    fn deactivate_subscriptions_expire_before(&self, before: i64) -> QueryResult<()> {
        diesel::update(subscriptions::table.filter(subscriptions::expires_at.le(before)))
            .set(subscriptions::expires_at.eq(None::<i64>))
            .execute(&self.inner)
            .map(|_| ())
    }

    fn delete_subscriptions(&self, id: u64) -> QueryResult<bool> {
        diesel::delete(subscriptions::table.find(id as i64))
            .execute(&self.inner)
            .map(|n| n != 0)
    }

    fn get_next_expiry(&self) -> QueryResult<Option<i64>> {
        subscriptions::table
            .select(subscriptions::expires_at)
            .filter(subscriptions::expires_at.is_not_null())
            .order(subscriptions::expires_at.asc())
            .first::<Option<i64>>(&self.inner)
            .optional()
            .map(Option::flatten)
    }
}

impl<C> Connection<C> {
    pub fn into_inner(self) -> C {
        self.inner
    }
}

impl<C> AsRef<C> for Connection<C> {
    fn as_ref(&self) -> &C {
        &self.inner
    }
}

impl<M: ManageConnection> Pool<M>
where
    PooledConnection<M>: diesel::Connection,
    <PooledConnection<M> as diesel::Connection>::Backend:
        Backend + HasSqlType<sql_types::Bool> + 'static,
    bool: FromSql<sql_types::Bool, <PooledConnection<M> as diesel::Connection>::Backend>,
    i64: FromSql<sql_types::BigInt, <PooledConnection<M> as diesel::Connection>::Backend>,
    *const str: FromSql<sql_types::Text, <PooledConnection<M> as diesel::Connection>::Backend>,
    ColumnInsertValue<subscriptions::id, Bound<sql_types::BigInt, i64>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
    for<'a> ColumnInsertValue<subscriptions::hub, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
    for<'a> ColumnInsertValue<subscriptions::topic, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
    for<'a> ColumnInsertValue<subscriptions::secret, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
{
    pub fn new(manager: M) -> Result<Self, diesel::r2d2::PoolError> {
        diesel::r2d2::Pool::new(manager).map(Pool::from)
    }
}

impl<M: ManageConnection> Pool<M> {
    pub fn into_inner(self) -> diesel::r2d2::Pool<M> {
        self.inner
    }
}

impl<M: ManageConnection> AsRef<diesel::r2d2::Pool<M>> for Pool<M> {
    fn as_ref(&self) -> &diesel::r2d2::Pool<M> {
        &self.inner
    }
}

impl<M: ManageConnection> From<diesel::r2d2::Pool<M>> for Pool<M>
where
    PooledConnection<M>: diesel::Connection,
    <PooledConnection<M> as diesel::Connection>::Backend:
        Backend + HasSqlType<sql_types::Bool> + 'static,
    bool: FromSql<sql_types::Bool, <PooledConnection<M> as diesel::Connection>::Backend>,
    i64: FromSql<sql_types::BigInt, <PooledConnection<M> as diesel::Connection>::Backend>,
    *const str: FromSql<sql_types::Text, <PooledConnection<M> as diesel::Connection>::Backend>,
    ColumnInsertValue<subscriptions::id, Bound<sql_types::BigInt, i64>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
    for<'a> ColumnInsertValue<subscriptions::hub, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
    for<'a> ColumnInsertValue<subscriptions::topic, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
    for<'a> ColumnInsertValue<subscriptions::secret, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
{
    fn from(pool: diesel::r2d2::Pool<M>) -> Self {
        Pool { inner: pool }
    }
}

impl<M: ManageConnection> Clone for Pool<M> {
    fn clone(&self) -> Self {
        Pool {
            inner: self.inner.clone(),
        }
    }
}

impl<M: ManageConnection> super::Pool for Pool<M>
where
    PooledConnection<M>: diesel::Connection,
    <PooledConnection<M> as diesel::Connection>::Backend:
        Backend + HasSqlType<sql_types::Bool> + 'static,
    bool: FromSql<sql_types::Bool, <PooledConnection<M> as diesel::Connection>::Backend>,
    i64: FromSql<sql_types::BigInt, <PooledConnection<M> as diesel::Connection>::Backend>,
    *const str: FromSql<sql_types::Text, <PooledConnection<M> as diesel::Connection>::Backend>,
    ColumnInsertValue<subscriptions::id, Bound<sql_types::BigInt, i64>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
    for<'a> ColumnInsertValue<subscriptions::hub, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
    for<'a> ColumnInsertValue<subscriptions::topic, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
    for<'a> ColumnInsertValue<subscriptions::secret, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, <PooledConnection<M> as diesel::Connection>::Backend>,
{
    type Connection = Connection<PooledConnection<M>>;
    type Error = diesel::r2d2::PoolError;

    fn get(&self) -> Result<Self::Connection, Self::Error> {
        self.inner.get().map(|inner| Connection { inner })
    }
}
