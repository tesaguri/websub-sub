#[cfg(feature = "diesel1")]
pub mod diesel1;

pub trait Connection {
    type Error;

    fn transaction<T, F>(&self, f: F) -> Result<T, Self::Error>
    where
        F: FnOnce() -> Result<T, Self::Error>;
    fn create_subscription(&self, hub: &str, topic: &str, secret: &str)
        -> Result<u64, Self::Error>;
    fn get_topic(&self, id: u64) -> Result<Option<(String, String)>, Self::Error>;
    fn subscription_exists(&self, id: u64, topic: &str) -> Result<bool, Self::Error>;
    // XXX: This method returns subscription IDs as `i64` unlike other methods that return `u64`.
    // That is because there is no way to convert a `Vec<(i64, ..)>` to `Vec<(u64, ..)>` both
    // efficiently and soundly. An efficient way would involve a type punning between tuple types,
    // which is always unsound.
    fn get_subscriptions_expire_before(
        &self,
        before: i64,
    ) -> Result<Vec<(i64, String, String)>, Self::Error>;
    fn get_hub_of_inactive_subscription(
        &self,
        id: u64,
        topic: &str,
    ) -> Result<Option<String>, Self::Error>;
    fn get_old_subscriptions(
        &self,
        id: u64,
        hub: &str,
        topic: &str,
    ) -> Result<Vec<u64>, Self::Error>;
    fn activate_subscription(&self, id: u64, expires_at: i64) -> Result<bool, Self::Error>;
    fn deactivate_subscriptions_expire_before(&self, before: i64) -> Result<(), Self::Error>;
    fn delete_subscriptions(&self, id: u64) -> Result<bool, Self::Error>;
    fn get_next_expiry(&self) -> Result<Option<i64>, Self::Error>;
}

pub trait Pool: Clone + Send + Sync + 'static {
    type Connection: Connection;
    type Error;

    fn get(&self) -> Result<Self::Connection, Self::Error>;
}
