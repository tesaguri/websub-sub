use diesel::prelude::*;
use futures::future;
use futures::stream::{FuturesUnordered, StreamExt, TryStreamExt};
use http::Uri;
use structopt::StructOpt;

use crate::hub;
use crate::schema::*;

#[derive(StructOpt)]
pub struct Opt {
    callback: Uri,
    hub: Option<String>,
    topic: String,
}

pub async fn main(opt: Opt) -> anyhow::Result<()> {
    let client = crate::common::http_client();
    let conn = crate::common::open_database()?;

    let tasks = if let Some(hub) = opt.hub {
        conn.transaction::<_, diesel::result::Error, _>(|| {
            let ids = subscriptions::table
                .filter(subscriptions::hub.eq(&hub))
                .filter(subscriptions::topic.eq(&opt.topic))
                .select(subscriptions::id)
                .load(&conn)?;
            let task = ids
                .into_iter()
                .map(|id| {
                    tokio::spawn(hub::unsubscribe(
                        &opt.callback,
                        id,
                        hub.clone(),
                        opt.topic.clone(),
                        client.clone(),
                        &conn,
                    ))
                })
                .collect::<FuturesUnordered<_>>();
            Ok(task)
        })?
    } else {
        hub::unsubscribe_all(&opt.callback, opt.topic, client, &conn)
            .map(tokio::spawn)
            .collect::<FuturesUnordered<_>>()
    };
    tasks
        .map(|result| result.unwrap())
        .try_for_each(|()| future::ok(()))
        .await?;

    Ok(())
}
