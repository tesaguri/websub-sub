#[cfg(feature = "diesel1")]
pub mod diesel1;
#[cfg(feature = "diesel2")]
pub mod diesel2;

// Our goal here is to abstract over both shared connections `&C` (Diesel 1) and exclusive
// connections `&mut C` (Diesel 2).
//
// To implement a `transaction` method for `&mut C`, the closure argument of the method need to take
// a reference to the connection itself since the closure cannot borrow the connection from the
// outer environment due to the exclusive borrow. However, implementing a `transaction` method for
// shared connections that way is tricky because the underlying `transaction` implementation
// nonexclusively borrows the connection, preventing the closure argument from being created.
//
// The problem can be solved by implementing the conenction trait for shared reference types `&T`
// instead of owned ones, so that we can just reborrow and then exclusively borrow the shared
// reference (`&mut &T`).
//
// But we also want the connection types returned by connection pools, which are typically owned,
// to be bound by the connection trait. So we divide the connection trait into two parts: the owned
// part `Connection` and the borrowed part `ConmectionRef`. The former is returned by pools and
// produces the latter, which does the real business.
//
// An alternative approach would be to implement a single connection trait for both of owned and
// borrowed connections, which we don't take because of a possible monomorphization bloat.

pub trait Connection {
    type Ref<'a>: ConnectionRef<Error = Self::Error>
    where
        Self: 'a;
    type Error;

    fn as_conn_ref(&mut self) -> Self::Ref<'_>;
}

pub trait ConnectionRef: Sized {
    type Reborrowed<'a>: ConnectionRef<Error = Self::Error>
    where
        Self: 'a;
    type Error;

    fn reborrow(&mut self) -> Self::Reborrowed<'_>;
    fn transaction<T, F>(&mut self, f: F) -> Result<T, Self::Error>
    where
        F: FnOnce(&mut Self) -> Result<T, Self::Error>;
    fn create_subscription(
        &mut self,
        hub: &str,
        topic: &str,
        secret: &str,
    ) -> Result<u64, Self::Error>;
    fn get_topic(&mut self, id: u64) -> Result<Option<(String, String)>, Self::Error>;
    fn subscription_exists(&mut self, id: u64, topic: &str) -> Result<bool, Self::Error>;
    fn get_subscriptions_expire_before(
        &mut self,
        before: i64,
    ) -> Result<Vec<(u64, String, String)>, Self::Error>;
    fn get_hub_of_inactive_subscription(
        &mut self,
        id: u64,
        topic: &str,
    ) -> Result<Option<String>, Self::Error>;
    fn get_old_subscriptions(
        &mut self,
        id: u64,
        hub: &str,
        topic: &str,
    ) -> Result<Vec<u64>, Self::Error>;
    fn activate_subscription(&mut self, id: u64, expires_at: i64) -> Result<bool, Self::Error>;
    fn deactivate_subscriptions_expire_before(&mut self, before: i64) -> Result<(), Self::Error>;
    fn delete_subscriptions(&mut self, id: u64) -> Result<bool, Self::Error>;
    fn get_next_expiry(&mut self) -> Result<Option<i64>, Self::Error>;
}

pub trait Pool: Clone + Send + Sync + 'static {
    type Connection: Connection;
    type Error;

    fn get(&self) -> Result<Self::Connection, Self::Error>;
}
