use std::mem;
use std::ops::Deref;
use std::ptr::NonNull;

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

pub struct TxConnection<C> {
    // This is essentially a lifetime-erased immutable reference to `C`.
    // Ideally, we'd like to make `<Connection<C> as super::Connection>::TxConnection` be
    // `&Connection<C>` but that would require GAT, hence the lifetime erasure.

    // Invariant:
    // The original connection `C` must outlive `conn`.
    conn: NonNull<C>,
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

    fn create_subscription(conn: &C, hub: &str, topic: &str, secret: &str) -> QueryResult<u64> {
        let mut rng = rand::thread_rng();
        conn.transaction(|| loop {
            let id = rng.next_u64();
            let result = diesel::insert_into(subscriptions::table)
                .values((
                    subscriptions::id.eq(id as i64),
                    subscriptions::hub.eq(hub),
                    subscriptions::topic.eq(topic),
                    subscriptions::secret.eq(secret),
                ))
                .execute(conn);
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

    fn get_topic(conn: &C, id: u64) -> QueryResult<Option<(String, String)>> {
        subscriptions::table
            .select((subscriptions::topic, subscriptions::secret))
            .find(id as i64)
            .get_result::<(String, String)>(conn)
            .optional()
    }

    fn subscription_exists(conn: &C, id: u64, topic: &str) -> QueryResult<bool> {
        diesel::select(exists(
            subscriptions::table
                .find(id as i64)
                .filter(subscriptions::topic.eq(topic)),
        ))
        .get_result(conn)
    }

    fn get_subscriptions_expire_before(
        conn: &C,
        before: i64,
    ) -> QueryResult<Vec<(i64, String, String)>> {
        subscriptions::table
            .filter(subscriptions::expires_at.le(before))
            .select((subscriptions::id, subscriptions::hub, subscriptions::topic))
            .load::<(i64, String, String)>(conn)
    }

    fn get_hub_of_inactive_subscription(
        conn: &C,
        id: u64,
        topic: &str,
    ) -> QueryResult<Option<String>> {
        subscriptions::table
            .find(id as i64)
            .filter(subscriptions::topic.eq(topic))
            .filter(subscriptions::expires_at.is_null())
            .select(subscriptions::hub)
            .get_result(conn)
            .optional()
    }

    fn get_old_subscriptions(conn: &C, id: u64, hub: &str, topic: &str) -> QueryResult<Vec<u64>> {
        let vec: Vec<i64> = subscriptions::table
            .filter(subscriptions::hub.eq(&hub))
            .filter(subscriptions::topic.eq(&topic))
            .filter(not(subscriptions::id.eq(id as i64)))
            .select(subscriptions::id)
            .load(conn)?;
        let mut vec = mem::ManuallyDrop::new(vec);
        let (ptr, length, capacity) = (vec.as_mut_ptr(), vec.len(), vec.capacity());
        // Safety: `u64` has the same size and alignment as `i64`.
        let vec = unsafe { Vec::from_raw_parts(ptr.cast::<u64>(), length, capacity) };
        Ok(vec)
    }

    fn activate_subscription(conn: &C, id: u64, expires_at: i64) -> QueryResult<bool> {
        diesel::update(subscriptions::table.find(id as i64))
            .set(subscriptions::expires_at.eq(expires_at))
            .execute(conn)
            .map(|n| n != 0)
    }

    fn deactivate_subscriptions_expire_before(conn: &C, before: i64) -> QueryResult<()> {
        diesel::update(subscriptions::table.filter(subscriptions::expires_at.le(before)))
            .set(subscriptions::expires_at.eq(None::<i64>))
            .execute(conn)
            .map(|_| ())
    }

    fn delete_subscriptions(conn: &C, id: u64) -> QueryResult<bool> {
        diesel::delete(subscriptions::table.find(id as i64))
            .execute(conn)
            .map(|n| n != 0)
    }

    fn get_next_expiry(conn: &C) -> QueryResult<Option<i64>> {
        subscriptions::table
            .select(subscriptions::expires_at)
            .filter(subscriptions::expires_at.is_not_null())
            .order(subscriptions::expires_at.asc())
            .first::<Option<i64>>(conn)
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

impl<C: diesel::Connection> super::Connection for Connection<C>
where
    C::Backend: Backend + HasSqlType<sql_types::Bool> + 'static,
    i64: FromSql<sql_types::BigInt, C::Backend>,
    bool: FromSql<sql_types::Bool, C::Backend>,
    *const str: FromSql<sql_types::Text, C::Backend>,
    ColumnInsertValue<subscriptions::id, Bound<sql_types::BigInt, i64>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::hub, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::topic, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::secret, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
{
    type TxConnection = TxConnection<C>;
    type Error = diesel::result::Error;

    fn transaction<T, F>(&mut self, f: F) -> Result<T, Self::Error>
    where
        F: FnOnce(&mut Self::TxConnection) -> Result<T, Self::Error>,
    {
        let conn = &self.inner;
        unsafe {
            // Safety:
            // - `tx` only lives for this block, which `conn` outlives.
            //   `f` cannot clone `tx` nor can it move `tx` outside the block.
            // - We have already borrowed `conn` immutably so `f` cannot borrow it mutably.
            let mut tx = TxConnection::new(conn);
            tx.transaction(f)
        }
    }

    fn create_subscription(&mut self, hub: &str, topic: &str, secret: &str) -> QueryResult<u64> {
        Self::create_subscription(&self.inner, hub, topic, secret)
    }

    fn get_topic(&mut self, id: u64) -> QueryResult<Option<(String, String)>> {
        Self::get_topic(&self.inner, id)
    }

    fn subscription_exists(&mut self, id: u64, topic: &str) -> QueryResult<bool> {
        Self::subscription_exists(&self.inner, id, topic)
    }

    fn get_subscriptions_expire_before(
        &mut self,
        before: i64,
    ) -> QueryResult<Vec<(i64, String, String)>> {
        Self::get_subscriptions_expire_before(&self.inner, before)
    }

    fn get_hub_of_inactive_subscription(
        &mut self,
        id: u64,
        topic: &str,
    ) -> QueryResult<Option<String>> {
        Self::get_hub_of_inactive_subscription(&self.inner, id, topic)
    }

    fn get_old_subscriptions(&mut self, id: u64, hub: &str, topic: &str) -> QueryResult<Vec<u64>> {
        Self::get_old_subscriptions(&self.inner, id, hub, topic)
    }

    fn activate_subscription(&mut self, id: u64, expires_at: i64) -> QueryResult<bool> {
        Self::activate_subscription(&self.inner, id, expires_at)
    }

    fn deactivate_subscriptions_expire_before(&mut self, before: i64) -> QueryResult<()> {
        Self::deactivate_subscriptions_expire_before(&self.inner, before)
    }

    fn delete_subscriptions(&mut self, id: u64) -> QueryResult<bool> {
        Self::delete_subscriptions(&self.inner, id)
    }

    fn get_next_expiry(&mut self) -> QueryResult<Option<i64>> {
        Self::get_next_expiry(&self.inner)
    }
}

impl<C> TxConnection<C> {
    /// ## Safety
    ///
    /// - `conn` must outlive the resulting `Self` value.
    /// - There must be no mutable reference to `conn` while `Self` lives.
    unsafe fn new(conn: &C) -> Self {
        TxConnection {
            conn: NonNull::from(conn),
        }
    }
}

impl<C> AsRef<C> for TxConnection<C> {
    fn as_ref(&self) -> &C {
        self
    }
}

impl<C> Deref for TxConnection<C> {
    type Target = C;

    fn deref(&self) -> &C {
        // Safety: The caller of `new` function guarantees validity of `&C`.
        unsafe { self.conn.as_ref() }
    }
}

impl<C: diesel::Connection> super::Connection for TxConnection<C>
where
    C::Backend: Backend + HasSqlType<sql_types::Bool> + 'static,
    i64: FromSql<sql_types::BigInt, C::Backend>,
    bool: FromSql<sql_types::Bool, C::Backend>,
    *const str: FromSql<sql_types::Text, C::Backend>,
    ColumnInsertValue<subscriptions::id, Bound<sql_types::BigInt, i64>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::hub, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::topic, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
    for<'a> ColumnInsertValue<subscriptions::secret, Bound<sql_types::Text, &'a str>>:
        InsertValues<subscriptions::table, C::Backend>,
{
    type TxConnection = Self;
    type Error = diesel::result::Error;

    fn transaction<T, F>(&mut self, f: F) -> Result<T, Self::Error>
    where
        F: FnOnce(&mut Self::TxConnection) -> Result<T, Self::Error>,
    {
        let conn = &**self;
        conn.transaction(|| unsafe {
            // Safety:
            // `tx` only lives for this block, which `self` outlives.
            // - `tx` only lives for this block, which `conn` outlives.
            //   `f` cannot clone `tx` nor can it move `tx` outside the block.
            // - We have already borrowed `conn` immutably so `f` cannot borrow it mutably.
            let mut tx = TxConnection::new(conn);
            f(&mut tx)
        })
    }

    fn create_subscription(&mut self, hub: &str, topic: &str, secret: &str) -> QueryResult<u64> {
        Connection::create_subscription(&**self, hub, topic, secret)
    }

    fn get_topic(&mut self, id: u64) -> QueryResult<Option<(String, String)>> {
        Connection::get_topic(&**self, id)
    }

    fn subscription_exists(&mut self, id: u64, topic: &str) -> QueryResult<bool> {
        Connection::subscription_exists(&**self, id, topic)
    }

    fn get_subscriptions_expire_before(
        &mut self,
        before: i64,
    ) -> Result<Vec<(i64, String, String)>, Self::Error> {
        Connection::get_subscriptions_expire_before(&**self, before)
    }

    fn get_hub_of_inactive_subscription(
        &mut self,
        id: u64,
        topic: &str,
    ) -> QueryResult<Option<String>> {
        Connection::get_hub_of_inactive_subscription(&**self, id, topic)
    }

    fn get_old_subscriptions(&mut self, id: u64, hub: &str, topic: &str) -> QueryResult<Vec<u64>> {
        Connection::get_old_subscriptions(&**self, id, hub, topic)
    }

    fn activate_subscription(&mut self, id: u64, expires_at: i64) -> QueryResult<bool> {
        Connection::activate_subscription(&**self, id, expires_at)
    }

    fn deactivate_subscriptions_expire_before(&mut self, before: i64) -> Result<(), Self::Error> {
        Connection::deactivate_subscriptions_expire_before(&**self, before)
    }

    fn delete_subscriptions(&mut self, id: u64) -> QueryResult<bool> {
        Connection::delete_subscriptions(&**self, id)
    }

    fn get_next_expiry(&mut self) -> Result<Option<i64>, Self::Error> {
        Connection::get_next_expiry(&**self)
    }
}

unsafe impl<C: Sync> Send for TxConnection<C> {}

unsafe impl<C: Sync> Sync for TxConnection<C> {}

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
