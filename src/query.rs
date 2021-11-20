use diesel::dsl::*;
use diesel::prelude::*;

use crate::schema::*;

pub fn expires_at() -> Order<
    Select<active_subscriptions::table, active_subscriptions::expires_at>,
    Asc<active_subscriptions::expires_at>,
> {
    active_subscriptions::table
        .select(active_subscriptions::expires_at)
        .order(active_subscriptions::expires_at.asc())
}
