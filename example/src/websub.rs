use crate::schema::*;

websub_sub::db::diesel2::define_connection! {
    subscriptions::table {
        id: subscriptions::id,
        hub: subscriptions::hub,
        topic: subscriptions::topic,
        secret: subscriptions::secret,
        expires_at: subscriptions::expires_at,
    }
}
