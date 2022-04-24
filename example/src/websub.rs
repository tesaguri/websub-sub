use crate::schema::*;

websub_sub::db::diesel1::define_connection! {
    subscriptions::table {
        id: subscriptions::id,
        hub: subscriptions::hub,
        topic: subscriptions::topic,
        secret: subscriptions::secret,
        expires_at: subscriptions::expires_at,
    }
}
