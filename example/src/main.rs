mod cmd;
mod common;

use structopt::StructOpt;

#[derive(StructOpt)]
enum Cmd {
    Run(cmd::run::Opt),
    Subscribe(cmd::subscribe::Opt),
    Unsubscribe(cmd::unsubscribe::Opt),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cmd = Cmd::from_args();
    match cmd {
        Cmd::Run(opt) => cmd::run::main(opt).await,
        Cmd::Subscribe(opt) => cmd::subscribe::main(opt).await,
        Cmd::Unsubscribe(opt) => cmd::unsubscribe::main(opt).await,
    }
}
