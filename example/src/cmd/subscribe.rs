use hyper::Uri;
use websub_sub::hub;

use crate::websub::Connection;

#[derive(clap::Args)]
pub struct Opt {
    callback: Uri,
    hub: String,
    topic: String,
}

pub async fn main(opt: Opt) -> anyhow::Result<()> {
    let client = crate::common::http_client();
    let mut conn = Connection::new(crate::common::open_database()?);

    hub::subscribe(&opt.callback, opt.hub, opt.topic, &client, &mut conn)?.await?;

    Ok(())
}
