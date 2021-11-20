use http::Uri;
use structopt::StructOpt;

use crate::hub;

#[derive(StructOpt)]
pub struct Opt {
    callback: Uri,
    hub: String,
    topic: String,
}

pub async fn main(opt: Opt) -> anyhow::Result<()> {
    let client = crate::common::http_client();
    let conn = crate::common::open_database()?;

    hub::subscribe(&opt.callback, opt.hub, opt.topic, &client, &conn).await?;

    Ok(())
}
