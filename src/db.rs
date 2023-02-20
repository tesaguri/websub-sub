#[cfg(feature = "diesel2")]
pub mod diesel2;

pub trait Connection {
    type Error;

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
