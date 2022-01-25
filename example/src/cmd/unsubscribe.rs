use diesel::prelude::*;
use futures::future;
use futures::stream::{FuturesUnordered, StreamExt, TryStreamExt};
use hyper::Uri;
use structopt::StructOpt;
use websub_sub::db::diesel1::Connection;
use websub_sub::db::Connection as _;
use websub_sub::hub;
use websub_sub::schema::*;

#[derive(StructOpt)]
pub struct Opt {
    callback: Uri,
    hub: Option<String>,
    topic: String,
}

pub async fn main(opt: Opt) -> anyhow::Result<()> {
    let client = crate::common::http_client();
    let mut conn = Connection::new(crate::common::open_database()?);

    let tasks: FuturesUnordered<_> = if let Some(hub) = opt.hub {
        conn.transaction(|conn| {
            let ids = subscriptions::table
                .filter(subscriptions::hub.eq(&hub))
                .filter(subscriptions::topic.eq(&opt.topic))
                .select(subscriptions::id)
                .load::<i64>(&**conn)?;
            ids.into_iter()
                .map(|id| {
                    hub::unsubscribe(
                        &opt.callback,
                        id as u64,
                        hub.clone(),
                        opt.topic.clone(),
                        client.clone(),
                        conn,
                    )
                    .map(tokio::spawn)
                })
                .collect()
        })?
    } else {
        conn.transaction(|conn| {
            let subscriptions = subscriptions::table
                .filter(subscriptions::topic.eq(&opt.topic))
                .select((subscriptions::id, subscriptions::hub))
                .load::<(i64, String)>(&**conn)?;
            subscriptions
                .into_iter()
                .map(|(id, hub)| {
                    hub::unsubscribe(
                        &opt.callback,
                        id as u64,
                        hub,
                        opt.topic.clone(),
                        client.clone(),
                        conn,
                    )
                    .map(tokio::spawn)
                })
                .collect()
        })?
    };
    tasks
        .map(Result::unwrap)
        .try_for_each(|()| future::ok(()))
        .await?;

    Ok(())
}
