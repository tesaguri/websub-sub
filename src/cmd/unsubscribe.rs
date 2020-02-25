use std::fmt::Write;

use diesel::prelude::*;
use structopt::StructOpt;

use crate::schema::subscriptions;
use crate::sub::{self, Sub};

#[derive(StructOpt)]
pub struct Opt {
    host: String,
    hub: String,
    topic: String,
}

pub async fn main(mut opt: Opt) {
    let conn = crate::common::open_database();
    let id: i64 = subscriptions::table
        .select(subscriptions::id)
        .filter(
            subscriptions::hub
                .eq(&opt.hub)
                .and(subscriptions::topic.eq(&opt.topic)),
        )
        .get_result(&conn)
        .unwrap();
    write!(opt.host, "/websub/callback/{}", id).unwrap();
    // TODO: transaction
    diesel::delete(subscriptions::table.find(id))
        .execute(&conn)
        .unwrap();
    sub::send(
        &opt.hub,
        &Sub::Unsubscribe {
            callback: &opt.host,
            topic: &opt.topic,
        },
    )
    .await;
}
