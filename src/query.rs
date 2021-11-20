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

pub fn renewing_subs() -> Select<renewing_subscriptions::table, renewing_subscriptions::old> {
    renewing_subscriptions::table.select(renewing_subscriptions::old)
}
