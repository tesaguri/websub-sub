#[macro_use]
extern crate diesel;

mod cmd;
mod common;
mod feed;
mod schema;
mod sub;
mod subscriber;

use structopt::StructOpt;

#[derive(StructOpt)]
enum Cmd {
    Run(cmd::run::Opt),
    Subscribe(cmd::subscribe::Opt),
    Unsubscribe(cmd::unsubscribe::Opt),
}

#[tokio::main]
async fn main() {
    let cmd = Cmd::from_args();
    match cmd {
        Cmd::Run(opt) => cmd::run::main(opt).await,
        Cmd::Subscribe(opt) => cmd::subscribe::main(opt).await,
        Cmd::Unsubscribe(opt) => cmd::unsubscribe::main(opt).await,
    }
}
