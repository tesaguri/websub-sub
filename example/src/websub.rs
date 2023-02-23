use crate::schema::*;

pub type Connection<C> = websub_sub::db::diesel2::Connection<
    C,
    subscriptions::id,
    subscriptions::hub,
    subscriptions::topic,
    subscriptions::secret,
    subscriptions::expires_at,
>;
pub type Pool<M> = websub_sub::db::diesel2::Pool<
    M,
    subscriptions::id,
    subscriptions::hub,
    subscriptions::topic,
    subscriptions::secret,
    subscriptions::expires_at,
>;
