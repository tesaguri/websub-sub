use structopt::StructOpt;

use crate::sub::{self, Sub};

#[derive(StructOpt)]
pub struct Opt {
    callback: String,
    hub: String,
    topic: String,
}

pub async fn main(opt: Opt) {
    sub::send(
        &opt.hub,
        &Sub::Unsubscribe {
            callback: &opt.callback,
            topic: &opt.topic,
        },
    )
    .await;
}
