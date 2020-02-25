use std::fmt::Write;
use std::str;

use diesel::prelude::*;
use diesel::result::DatabaseErrorKind;
use rand::RngCore;
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
    let mut rng = rand::thread_rng();

    let secret = gen_secret(&mut rng);
    let secret = unsafe { str::from_utf8_unchecked(&secret) };

    // TODO: transaction
    let id = loop {
        let id = (rng.next_u64() >> 1) as i64;
        let result = diesel::replace_into(subscriptions::table)
            .values((
                subscriptions::id.eq(id),
                subscriptions::hub.eq(&opt.hub),
                subscriptions::topic.eq(&opt.topic),
                subscriptions::secret.eq(secret),
            ))
            .execute(&conn);
        match result {
            Ok(_) => break id,
            Err(diesel::result::Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                // retry
            }
            Err(e) => panic!("{:?}", e),
        }
    };

    write!(opt.host, "/websub/callback/{}", id).unwrap();
    sub::send(
        &opt.hub,
        &Sub::Subscribe {
            callback: &opt.host,
            topic: &opt.topic,
            secret,
        },
    )
    .await;
}

const SECRET_LEN: usize = 32;

fn gen_secret<R: RngCore>(mut rng: R) -> [u8; SECRET_LEN] {
    let mut ret = [0u8; SECRET_LEN];

    let mut rand = [0u8; SECRET_LEN * 6 / 8];
    rng.fill_bytes(&mut rand);

    let config = base64::Config::new(base64::CharacterSet::UrlSafe, false);
    base64::encode_config_slice(&rand, config, &mut ret);

    ret
}
