use http::Uri;
use structopt::StructOpt;

use crate::sub;

#[derive(StructOpt)]
pub struct Opt {
    host: Uri,
    hub: String,
    topic: String,
}

pub async fn main(opt: Opt) {
    let client = crate::common::http_client();
    let conn = crate::common::open_database();

    sub::unsubscribe_all(&opt.host, &opt.hub, &opt.topic, &client, &conn).await;
}
