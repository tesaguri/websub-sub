#[doc(inline)]
pub use crate::diesel1_define_connection as define_connection;

// XXX: Unlike `crate::db::diesel2`, this is implemented as a macro instead of generics because the
// required trait bounds around the schema types would involve too many private types.
// Also, we could think of a version of this macro which does not take `$table` argument and uses
// `<$id as Column>::Table` in place of `$table`, but in practice, that would somehow confuse rustc
// while resolving the requirement of `ColumnInsertValue<id, Bound<..>>: InsertValues<table, _>`.
#[macro_export]
#[doc(hidden)]
macro_rules! diesel1_define_connection {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
            $table { $($column: $path,)* }
            pub(crate) Connection;
            pub(crate) Pool;
        }
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! _diesel1_define_connection_inner {
    // Entry point:
    (
        $table:path { $($column:tt: $path:path),* $(,)? }
        $(#[$cattr:meta])* $cvis:vis $Connection:ident;
        $(#[$pattr:meta])* $pvis:vis $Pool:ident;
    ) => {
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner!(@duplicate id);
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner!(@duplicate hub);
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner!(@duplicate topic);
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner!(@duplicate secret);
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner!(@duplicate expires_at);
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner! {
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
        $crate::_diesel1_define_connection_inner!(@missing $table, id);
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
        $crate::_diesel1_define_connection_inner!(@missing $table, hub);
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
        $crate::_diesel1_define_connection_inner!(@missing $table, topic);
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
        $crate::_diesel1_define_connection_inner!(@missing $table, secret);
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
        $crate::_diesel1_define_connection_inner!(@missing $table, expires_at);
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
        $cvis struct $Connection<C> {
            inner: C,
        }

        $(#[$pattr])*
        $pvis struct $Pool<M>
        where
            M: $crate::_private::diesel::r2d2::ManageConnection,
        {
            inner: $crate::_private::diesel::r2d2::Pool<M>,
        }

        const _: () = {
            use $crate::_private::diesel::prelude::*;

            impl<_C: $crate::_private::diesel::Connection> $Connection<_C>
            where
                _C::Backend: $crate::_private::diesel::sql_types::HasSqlType<$crate::_private::diesel::sql_types::Bool> + 'static,
                i64: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::BigInt, _C::Backend>,
                bool: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Bool, _C::Backend>,
                String: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Text, _C::Backend>,
                // XXX: Can we remove these `#[doc(hidden)]` types? These bounds are required for
                // `insert_into(table).values((id.eq(_), hub.eq(_), topic.eq(_), secret.eq(_)))`
                // to implement `ExecuteDsl<C>`. These traits are implemented differently between
                // SQLite and other backends and there seems to be no concise way to be generic over
                // them.
                $crate::_private::diesel::insertable::ColumnInsertValue<$id, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::BigInt, i64>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$hub, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$topic, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$secret, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
            {
                pub fn new(connection: _C) -> Self {
                    Self { inner: connection }
                }
            }

            impl<_C: $crate::_private::diesel::Connection> $crate::db::Connection for $Connection<_C>
            where
                _C::Backend: $crate::_private::diesel::sql_types::HasSqlType<$crate::_private::diesel::sql_types::Bool> + 'static,
                i64: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::BigInt, _C::Backend>,
                bool: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Bool, _C::Backend>,
                String: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Text, _C::Backend>,
                $crate::_private::diesel::insertable::ColumnInsertValue<$id, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::BigInt, i64>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$hub, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$topic, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$secret, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
            {
                type Ref<'a> = &'a Self where Self: 'a;
                type Error = $crate::_private::diesel::result::Error;

                fn as_conn_ref(&mut self) -> Self::Ref<'_> {
                    self
                }
            }

            impl<_C: $crate::_private::diesel::Connection> $crate::db::ConnectionRef for &$Connection<_C>
            where
                _C::Backend: $crate::_private::diesel::sql_types::HasSqlType<$crate::_private::diesel::sql_types::Bool> + 'static,
                i64: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::BigInt, _C::Backend>,
                bool: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Bool, _C::Backend>,
                String: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Text, _C::Backend>,
                $crate::_private::diesel::insertable::ColumnInsertValue<$id, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::BigInt, i64>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$hub, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$topic, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$secret, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, _C::Backend>,
            {
                type Reborrowed<'a> = &'a $Connection<_C> where Self: 'a;
                type Error = $crate::_private::diesel::result::Error;

                fn reborrow(&mut self) -> Self::Reborrowed<'_> {
                    self
                }

                fn transaction<T, F>(&mut self, f: F) -> Result<T, Self::Error>
                where
                    F: FnOnce(&mut Self) -> Result<T, Self::Error>,
                {
                    self.inner.transaction(|| f(self))
                }

                fn create_subscription(&mut self, hub: &str, topic: &str, secret: &str)
                    -> Result<u64, Self::Error>
                {
                    let mut rng = $crate::_private::rand::thread_rng();
                    self.inner.transaction(|| loop {
                        let id = $crate::_private::rand::RngCore::next_u64(&mut rng);
                        let result = $crate::_private::diesel::insert_into($table)
                            .values((
                                $id.eq(id as i64),
                                $hub.eq(hub),
                                $topic.eq(topic),
                                $secret.eq(secret),
                            ))
                            .execute(&self.inner);
                        match result {
                            Ok(_) => return Ok(id),
                            Err($crate::_private::diesel::result::Error::DatabaseError(
                                $crate::_private::diesel::result::DatabaseErrorKind::UniqueViolation,
                                _,
                            )) => {} // retry
                            Err(e) => return Err(e),
                        }
                    })
                }

                fn get_topic(&mut self, id: u64) -> Result<Option<(String, String)>, Self::Error> {
                    $table
                        .select(($topic, $secret))
                        .find(id as i64)
                        .get_result::<(String, String)>(&self.inner)
                        .optional()
                }

                fn subscription_exists(&mut self, id: u64, topic: &str) -> Result<bool, Self::Error> {
                    $crate::_private::diesel::select($crate::_private::diesel::dsl::exists(
                        $table
                            .find(id as i64)
                            .filter($topic.eq(topic)),
                    )).get_result(&self.inner)
                }

                fn get_subscriptions_expire_before(
                    &mut self,
                    before: i64,
                ) -> Result<Vec<(u64, String, String)>, Self::Error> {
                    type Ret = (u64, String, String);
                    // Define a type that can be serialized from `(i64, String, String)` and soundly
                    // transmuted into `Ret`. Note that we are not serializing into
                    // `(i64, String, String)` directly because the tuple types have the default
                    // representation which cannot be transmuted soundly into each other.
                    #[repr(transparent)]
                    struct Subscription(Ret);

                    impl<DB: $crate::_private::diesel::backend::Backend> $crate::_private::diesel::Queryable<
                        ($crate::_private::diesel::sql_types::BigInt, $crate::_private::diesel::sql_types::Text, $crate::_private::diesel::sql_types::Text),
                        DB,
                    > for Subscription
                    where
                        i64: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::BigInt, DB>,
                        String: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Text, DB>,
                    {
                        type Row = (i64, String, String);

                        fn build((id, hub, topic): Self::Row) -> Self {
                            Subscription((id as u64, hub, topic))
                        }
                    }

                    let vec: Vec<Subscription> = $table
                        .filter($expires_at.le(before))
                        .select(($id, $hub, $topic))
                        .load(&self.inner)?;

                    let mut vec = $crate::_private::core::mem::ManuallyDrop::new(vec);
                    let (ptr, length, capacity) = (vec.as_mut_ptr(), vec.len(), vec.capacity());
                    let vec = unsafe {
                        // SAFETY:
                        // The `#[repr(transparent)]` attribute on `Subscription` declaration
                        // ensures that it has the same layout as `Ret`.
                        Vec::from_raw_parts(ptr.cast::<Ret>(), length, capacity)
                    };
                    Ok(vec)
                }

                fn get_hub_of_inactive_subscription(
                    &mut self,
                    id: u64,
                    topic: &str,
                ) -> Result<Option<String>, Self::Error> {
                    $table
                        .find(id as i64)
                        .filter($topic.eq(topic))
                        .filter($expires_at.is_null())
                        .select($hub)
                        .get_result(&self.inner)
                        .optional()
                }

                fn get_old_subscriptions(&mut self, id: u64, hub: &str, topic: &str)
                    -> Result<Vec<u64>, Self::Error>
                {
                    let vec: Vec<i64> = $table
                        .filter($hub.eq(&hub))
                        .filter($topic.eq(&topic))
                        .filter($crate::_private::diesel::dsl::not($id.eq(id as i64)))
                        .select($id)
                        .load(&self.inner)?;

                    let mut vec = $crate::_private::core::mem::ManuallyDrop::new(vec);
                    let (ptr, length, capacity) = (vec.as_mut_ptr(), vec.len(), vec.capacity());
                    let vec = unsafe {
                        // SAFETY:
                        // `u64` has the same layout as `i64`.
                        Vec::from_raw_parts(ptr.cast::<u64>(), length, capacity)
                    };
                    Ok(vec)
                }

                fn activate_subscription(&mut self, id: u64, expires_at: i64) -> Result<bool, Self::Error> {
                    $crate::_private::diesel::update($table.find(id as i64))
                        .set($expires_at.eq(expires_at))
                        .execute(&self.inner)
                        .map(|n| n != 0)
                }

                fn deactivate_subscriptions_expire_before(&mut self, before: i64) -> Result<(), Self::Error> {
                    $crate::_private::diesel::update($table.filter($expires_at.le(before)))
                        .set($expires_at.eq(None::<i64>))
                        .execute(&self.inner)
                        .map(|_| ())
                }

                fn delete_subscriptions(&mut self, id: u64) -> Result<bool, Self::Error> {
                    $crate::_private::diesel::delete($table.find(id as i64))
                        .execute(&self.inner)
                        .map(|n| n != 0)
                }

                fn get_next_expiry(&mut self) -> Result<Option<i64>, Self::Error> {
                    $table
                        .select($expires_at)
                        .filter($expires_at.is_not_null())
                        .order($expires_at.asc())
                        .first::<Option<i64>>(&self.inner)
                        .optional()
                        .map(Option::flatten)
                }
            }

            impl<_C> $Connection<_C> {
                pub fn into_inner(self) -> _C {
                    self.inner
                }
            }

            impl<_C> AsRef<_C> for $Connection<_C> {
                fn as_ref(&self) -> &_C {
                    &self.inner
                }
            }

            impl<_C> AsMut<_C> for $Connection<_C> {
                fn as_mut(&mut self) -> &mut _C {
                    &mut self.inner
                }
            }

            impl<_M: $crate::_private::diesel::r2d2::ManageConnection> $Pool<_M>
            where
                $crate::_private::diesel::r2d2::PooledConnection<_M>: $crate::_private::diesel::Connection,
                <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend: $crate::_private::diesel::sql_types::HasSqlType<$crate::_private::diesel::sql_types::Bool> + 'static,
                bool: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Bool, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                i64: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::BigInt, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                String: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Text, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                $crate::_private::diesel::insertable::ColumnInsertValue<$id, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::BigInt, i64>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$hub, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$topic, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$secret, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
            {
                pub fn new(manager: _M) -> Result<Self, $crate::_private::diesel::r2d2::PoolError> {
                    $crate::_private::diesel::r2d2::Pool::new(manager).map($Pool::from)
                }
            }

            impl<_M: $crate::_private::diesel::r2d2::ManageConnection> $Pool<_M> {
                pub fn into_inner(self) -> $crate::_private::diesel::r2d2::Pool<_M> {
                    self.inner
                }
            }

            impl<_M: $crate::_private::diesel::r2d2::ManageConnection> AsRef<$crate::_private::diesel::r2d2::Pool<_M>> for $Pool<_M> {
                fn as_ref(&self) -> &$crate::_private::diesel::r2d2::Pool<_M> {
                    &self.inner
                }
            }

            impl<_M: $crate::_private::diesel::r2d2::ManageConnection> From<$crate::_private::diesel::r2d2::Pool<_M>> for $Pool<_M>
            where
                $crate::_private::diesel::r2d2::PooledConnection<_M>: $crate::_private::diesel::Connection,
                <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend: $crate::_private::diesel::sql_types::HasSqlType<$crate::_private::diesel::sql_types::Bool> + 'static,
                bool: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Bool, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                i64: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::BigInt, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                String: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Text, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                $crate::_private::diesel::insertable::ColumnInsertValue<$id, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::BigInt, i64>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$hub, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$topic, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$secret, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
            {
                fn from(pool: $crate::_private::diesel::r2d2::Pool<_M>) -> Self {
                    Self { inner: pool }
                }
            }

            impl<_M: $crate::_private::diesel::r2d2::ManageConnection> Clone for $Pool<_M> {
                fn clone(&self) -> Self {
                    Self {
                        inner: self.inner.clone(),
                    }
                }
            }

            impl<_M: $crate::_private::diesel::r2d2::ManageConnection> $crate::db::Pool for $Pool<_M>
            where
                $crate::_private::diesel::r2d2::PooledConnection<_M>: $crate::_private::diesel::Connection,
                <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend: $crate::_private::diesel::sql_types::HasSqlType<$crate::_private::diesel::sql_types::Bool> + 'static,
                bool: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Bool, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                i64: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::BigInt, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                String: $crate::_private::diesel::deserialize::FromSql<$crate::_private::diesel::sql_types::Text, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                $crate::_private::diesel::insertable::ColumnInsertValue<$id, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::BigInt, i64>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$hub, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$topic, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
                for<'a> $crate::_private::diesel::insertable::ColumnInsertValue<$secret, $crate::_private::diesel::expression::bound::Bound<$crate::_private::diesel::sql_types::Text, &'a str>>: $crate::_private::diesel::insertable::InsertValues<$table, <$crate::_private::diesel::r2d2::PooledConnection<_M> as $crate::_private::diesel::Connection>::Backend>,
            {
                type Connection = $Connection<$crate::_private::diesel::r2d2::PooledConnection<_M>>;
                type Error = $crate::_private::diesel::r2d2::PoolError;

                fn get(&self) -> Result<Self::Connection, Self::Error> {
                    self.inner.get().map(|inner| $Connection { inner })
                }
            }
        };
    };
}

#[cfg(all(test, not(feature = "diesel2")))]
mod tests {
    use crate::schema::*;

    use super::*;

    #[allow(unused)]
    fn it_compiles() {
        define_connection! {
            subscriptions::table {
                id: subscriptions::id,
                hub: subscriptions::hub,
                topic: subscriptions::topic,
                secret: subscriptions::secret,
                expires_at: subscriptions::expires_at,
            }
            connection C;
            pool M;
        }
    }
}
