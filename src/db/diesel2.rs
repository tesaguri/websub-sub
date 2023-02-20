#[doc(inline)]
pub use crate::diesel2_define_connection as define_connection;

// XXX: We could think of a version of this macro which does not take `$table` argument and uses
// `<$id as Column>::Table` in place of `$table`. In practice, however, that would somehow confuse
// rustc while resolving requirement of `ColumnInsertValue<id, Bound<..>>: InsertValues<table, _>`.
// Also, `{table_name}::table` type generated by `diesel::table!` does not implement `Default` so we
// would have no way to instanciate `$id::Table` anyway.
#[macro_export]
macro_rules! diesel2_define_connection {
    // This may not be the most concise way of defining a macro of this sort (you could do
    // something like `$($vis1 $kw1 $Name1; $($vis2 $kw2 $Name2;)?)?` for example), but the
    // documentation is the easiest to understand this way.
    (
        $table:path {
            $($column:tt: $path:path),* $(,)?
        }
        $(#[$conn_attr:meta])*
        $conn_vis:vis connection $Connection:ident;
        $(#[$pool_attr:meta])*
        $pool_vis:vis pool $Pool:ident;
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($column: $path,)* }
            $(#[$conn_attr])* $conn_vis $Connection;
            $(#[$pool_attr])* $pool_vis $Pool;
        }
    };
    (
        $table:path {
            $($column:tt: $path:path),* $(,)?
        }
        $(#[$pool_attr:meta])*
        $pool_vis:vis pool $Pool:ident;
        $(#[$conn_attr:meta])*
        $conn_vis:vis connection $Connection:ident;
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($column: $path,)* }
            $(#[$conn_attr])* $conn_vis $Connection;
            $(#[$pool_attr])* $pool_vis $Pool;
        }
    };
    (
        $table:path {
            $($column:tt: $path:path),* $(,)?
        }
        $(#[$conn_attr:meta])*
        $conn_vis:vis connection $Connection:ident;
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($column: $path,)* }
            $(#[$conn_attr])* $conn_vis $Connection;
            pub(crate) Pool;
        }
    };
    (
        $table:path {
            $($column:tt: $path:path),* $(,)?
        }
        $(#[$pool_attr:meta])*
        $pool_vis:vis pool $Pool:ident;
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($column: $path,)* }
            pub(crate) Connection;
            $(#[$pool_attr])* $pool_vis $Pool;
        }
    };
    (
        $table:path {
            $($column:tt: $path:path),* $(,)?
        }
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($column: $path,)* }
            pub(crate) Connection;
            pub(crate) Pool;
        }
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! _diesel2_define_connection_inner {
    // Entry point:
    (
        $table:path { $($column:tt: $path:path),* $(,)? }
        $(#[$cattr:meta])* $cvis:vis $Connection:ident;
        $(#[$pattr:meta])* $pvis:vis $Pool:ident;
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($column: $path,)* }
            id: @,
            hub: @,
            topic: @,
            secret: @,
            expires_at: @,
            $(#[$cattr])* $cvis $Connection,
            $(#[$pattr])* $pvis $Pool,
        }
    };
    // Parse columns:
    (
        $table:path { id: $id:path, $($rest:tt)* }
        id: @,
        hub: $hub:tt,
        topic: $topic:tt,
        secret: $secret:tt,
        expires_at: $expires_at:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    (
        $table:path { hub: $hub:path, $($rest:tt)* }
        id: $id:tt,
        hub: @,
        topic: $topic:tt,
        secret: $secret:tt,
        expires_at: $expires_at:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    (
        $table:path { topic: $topic:path, $($rest:tt)* }
        id: $id:tt,
        hub: $hub:tt,
        topic: @,
        secret: $secret:tt,
        expires_at: $expires_at:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    (
        $table:path { secret: $secret:path, $($rest:tt)* }
        id: $id:tt,
        hub: $hub:tt,
        topic: $topic:tt,
        secret: @,
        expires_at: $expires_at:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    (
        $table:path { expires_at: $expires_at:path, $($rest:tt)* }
        id: $id:tt,
        hub: $hub:tt,
        topic: $topic:tt,
        secret: $secret:tt,
        expires_at: @,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    // Handle erroneous inputs:
    // Duplicate columns:
    (
        $table:path { id: $_:path, $($rest:tt)* }
        id: $id:path,
        hub: $hub:tt,
        topic: $topic:tt,
        secret: $secret:tt,
        expires_at: $expires_at:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@duplicate id);
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    (
        $table:path { hub: $_:path, $($rest:tt)* }
        id: $id:tt,
        hub: $hub:path,
        topic: $topic:tt,
        secret: $secret:tt,
        expires_at: $expires_at:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@duplicate hub);
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    (
        $table:path { topic: $_:path, $($rest:tt)* }
        id: $id:tt,
        hub: $hub:tt,
        topic: $topic:path,
        secret: $secret:tt,
        expires_at: $expires_at:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@duplicate topic);
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    (
        $table:path { secret: $_:path, $($rest:tt)* }
        id: $id:tt,
        hub: $hub:tt,
        topic: $topic:tt,
        secret: $secret:path,
        expires_at: $expires_at:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@duplicate secret);
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    (
        $table:path { expires_at: $_:path, $($rest:tt)* }
        id: $id:tt,
        hub: $hub:tt,
        topic: $topic:tt,
        secret: $secret:tt,
        expires_at: $expires_at:path,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@duplicate expires_at);
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
            $($items)+
        }
    };
    (@duplicate $column:ident) => {
        compile_error!(concat!("column for `", stringify!($column), "` specified more than once"));
    };
    // Unknown columns:
    (
        $table:path { $column:ident: $_:path, $($rest:tt)* }
        id: $id:tt,
        hub: $hub:tt,
        topic: $topic:tt,
        secret: $secret:tt,
        expires_at: $expires_at:tt,
        $($items:tt)+
    ) => {
        compile_error!(concat!("Unknown colum kind `", stringify!($column), "`"));
        $crate::_diesel2_define_connection_inner! {
            $table { $($rest)* }
            id: $id,
            hub: $hub,
            topic: $topic,
            secret: $secret,
            expires_at: $expires_at,
        }
    };
    // Missing columns:
    (
        $table:path {}
        id: @,
        hub: $_h:tt,
        topic: $_t:tt,
        secret: $_s:tt,
        expires_at: $_e:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@missing $table, id);
    };
    (
        $table:path {}
        id: $_i:tt,
        hub: @,
        topic: $_t:tt,
        secret: $_s:tt,
        expires_at: $_e:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@missing $table, hub);
    };
    (
        $table:path {}
        id: $_i:tt,
        hub: $_h:tt,
        topic: @,
        secret: $_s:tt,
        expires_at: $_e:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@missing $table, topic);
    };
    (
        $table:path {}
        id: $_i:tt,
        hub: $_h:tt,
        topic: $_t:tt,
        secret: @,
        expires_at: $_e:tt,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@missing $table, secret);
    };
    (
        $table:path {}
        id: $_i:tt,
        hub: $_h:tt,
        topic: $_t:tt,
        secret: $_s:tt,
        expires_at: @,
        $($items:tt)+
    ) => {
        $crate::_diesel2_define_connection_inner!(@missing $table, expires_at);
    };
    (@missing $table:path, $column:ident) => {
        compile_error!(concat!(
            "missing column for `",
            stringify!($column),
            "` in table `",
            stringify!($table),
            "`",
        ));
    };
    // Expand to output:
    (
        $table:path {}
        id: $id:path,
        hub: $hub:path,
        topic: $topic:path,
        secret: $secret:path,
        expires_at: $expires_at:path,
        $(#[$cattr:meta])* $cvis:vis $Connection:ident,
        $(#[$pattr:meta])* $pvis:vis $Pool:ident,
    ) => {
        $(#[$cattr])*
        #[repr(transparent)]
        $cvis struct $Connection<C> {
            inner: C,
        }

        $(#[$pattr])*
        $pvis struct $Pool<M>
        where
            M: $crate::_private::diesel2::r2d2::ManageConnection,
        {
            inner: $crate::_private::diesel2::r2d2::Pool<M>,
        }

        const _: () = {
            use std::mem;

            use $crate::_private::diesel2 as diesel;
            use $crate::_private::rand;

            use diesel::backend::{Backend, DieselReserveSpecialization, SqlDialect};
            use diesel::backend::sql_dialect::exists_syntax::AnsiSqlExistsSyntax;
            use diesel::backend::sql_dialect::from_clause_syntax::AnsiSqlFromClauseSyntax;
            use diesel::backend::sql_dialect::select_statement_syntax::AnsiSqlSelectStatement;
            use diesel::connection::{DefaultLoadingMode, LoadConnection};
            use diesel::deserialize::FromSql;
            use diesel::dsl::*;
            use diesel::prelude::*;
            use diesel::query_builder::{DefaultValues, LimitOffsetClause, NoLimitClause, NoOffsetClause, QueryFragment};
            use diesel::r2d2::{ManageConnection, PooledConnection};
            use diesel::serialize::ToSql;
            use diesel::sql_types::{self, HasSqlType};
            use rand::RngCore;

            use self::{Connection, Pool};

            impl<C, DB> Connection<C>
            where
                C: diesel::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
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
            {
                pub fn new(connection: C) -> Self {
                    Connection { inner: connection }
                }
            }

            impl<C, DB> $crate::db::Connection for Connection<C>
            where
                C: diesel::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
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
            {
                type Error = diesel::result::Error;

                fn transaction<T, F>(&mut self, f: F) -> Result<T, Self::Error>
                where
                    F: FnOnce(&mut Self) -> Result<T, Self::Error>,
                {
                    self.inner.transaction(|inner| {
                        let this = unsafe {
                            // SAFETY:
                            // The `#[repr(transparent)]` attribute on `Connection<C>` declaration
                            // guarantees that `Connection<C>` has the same layout as `C`, ensuring
                            // the soundness of the transmution.
                            mem::transmute::<&mut C, &mut Connection<C>>(inner)
                        };
                        f(this)
                    })
                }

                fn create_subscription(&mut self, hub: &str, topic: &str, secret: &str)
                    -> QueryResult<u64>
                {
                    let mut rng = rand::thread_rng();
                    self.transaction(|this| loop {
                        let id = rng.next_u64();
                        let result = diesel::insert_into($table)
                            .values((
                                $id.eq(id as i64),
                                $hub.eq(hub),
                                $topic.eq(topic),
                                $secret.eq(secret),
                            ))
                            .execute(&mut this.inner);
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

                fn get_topic(&mut self, id: u64) -> QueryResult<Option<(String, String)>> {
                    $table
                        .select(($topic, $secret))
                        .find(id as i64)
                        .get_result::<(String, String)>(&mut self.inner)
                        .optional()
                }

                fn subscription_exists(&mut self, id: u64, topic: &str) -> QueryResult<bool> {
                    diesel::select(exists(
                        $table
                            .find(id as i64)
                            .filter($topic.eq(topic)),
                    ))
                    .get_result(&mut self.inner)
                }

                fn get_subscriptions_expire_before(
                    &mut self,
                    before: i64,
                ) -> QueryResult<Vec<(u64, String, String)>> {
                    $table
                        .filter($expires_at.le(before))
                        .select(($id, $hub, $topic))
                        .load_iter::<(i64, String, String), _>(&mut self.inner)?
                        .map(|result| result.map(|(id, hub, topic)| (id as u64, hub, topic)))
                        .collect()
                }

                fn get_hub_of_inactive_subscription(
                    &mut self,
                    id: u64,
                    topic: &str,
                ) -> QueryResult<Option<String>> {
                    $table
                        .find(id as i64)
                        .filter($topic.eq(topic))
                        .filter($expires_at.is_null())
                        .select($hub)
                        .get_result(&mut self.inner)
                        .optional()
                }

                fn get_old_subscriptions(&mut self, id: u64, hub: &str, topic: &str)
                    -> QueryResult<Vec<u64>>
                {
                    $table
                        .filter($hub.eq(&hub))
                        .filter($topic.eq(&topic))
                        .filter(not($id.eq(id as i64)))
                        .select($id)
                        .load_iter(&mut self.inner)?
                        .map(|result| result.map(|id: i64| id as u64))
                        .collect()
                }

                fn activate_subscription(&mut self, id: u64, expires_at: i64) -> QueryResult<bool> {
                    diesel::update($table.find(id as i64))
                        .set($expires_at.eq(expires_at))
                        .execute(&mut self.inner)
                        .map(|n| n != 0)
                }

                fn deactivate_subscriptions_expire_before(&mut self, before: i64) -> QueryResult<()> {
                    diesel::update($table.filter($expires_at.le(before)))
                        .set($expires_at.eq(None::<i64>))
                        .execute(&mut self.inner)
                        .map(|_| ())
                }

                fn delete_subscriptions(&mut self, id: u64) -> QueryResult<bool> {
                    diesel::delete($table.find(id as i64))
                        .execute(&mut self.inner)
                        .map(|n| n != 0)
                }

                fn get_next_expiry(&mut self) -> QueryResult<Option<i64>> {
                    $table
                        .filter($expires_at.is_not_null())
                        .select($expires_at.assume_not_null())
                        .order($expires_at.asc())
                        // XXX: Not using `first()`, which involves a `LIMIT` clause that introduces
                        // a private type `Bound<..>` to the trait bound puzzle
                        // (`LimitOffsetClause<LimitClause<Bound<BigInt, i64>>, NoOffsetClause>: QueryFragment<DB>`).
                        .load_iter(&mut self.inner)?
                        .next()
                        .transpose()
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

            impl<C> AsMut<C> for Connection<C> {
                fn as_mut(&mut self) -> &mut C {
                    &mut self.inner
                }
            }

            impl<M: ManageConnection, DB> Pool<M>
            where
                PooledConnection<M>:
                    diesel::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
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

            impl<M: ManageConnection, DB> From<diesel::r2d2::Pool<M>> for Pool<M>
            where
                PooledConnection<M>:
                    diesel::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
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

            impl<M: ManageConnection, DB> $crate::db::Pool for Pool<M>
            where
                PooledConnection<M>:
                    diesel::Connection<Backend = DB> + LoadConnection<DefaultLoadingMode>,
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
            {
                type Connection = Connection<PooledConnection<M>>;
                type Error = diesel::r2d2::PoolError;

                fn get(&self) -> Result<Self::Connection, Self::Error> {
                    self.inner.get().map(|inner| Connection { inner })
                }
            }
        };
    };
}