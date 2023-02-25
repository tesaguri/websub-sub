use hyper::header;
use hyper::Uri;
use websub_sub::hub;

use crate::feed::{self, RawFeed};
use crate::websub::Connection;

#[derive(clap::Args)]
pub struct Opt {
    callback: Uri,
    hub: Option<String>,
    topic: String,
}

pub async fn main(opt: Opt) -> anyhow::Result<()> {
    let client = crate::common::http_client();
    let mut conn = Connection::new(crate::common::open_database()?);

    if let Some(hub) = opt.hub {
        hub::subscribe(&opt.callback, hub, opt.topic, &client, &mut conn)?.await?;
    } else {
        let res = client.get((&opt.topic[..]).try_into()?).await?;
        if res.status() != hyper::StatusCode::OK {
            anyhow::bail!("Bad status: {}", res.status());
        }

        let mime = res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(feed::MediaType::Xml);

        let body = hyper::body::to_bytes(res.into_body()).await?;
        let feed::Meta { topic, hubs, .. } = RawFeed::parse(mime, &body).unwrap().take_meta();
        let topic = topic.as_deref().unwrap_or(&opt.topic);
        for hub in hubs {
            hub::subscribe(
                &opt.callback,
                hub.to_string(),
                topic.to_owned(),
                &client,
                &mut conn,
            )?
            .await?;
        }
    }

    Ok(())
}
