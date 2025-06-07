use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use middleware::magick::check_magick;
use tracing::{error, info};

use crate::middleware::{
    magick::{clear_tmp_dir, create_tmp_dir},
    runner::runner,
};

mod error;
mod middleware;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    drop(dotenvy::dotenv());

    let _guard = sentry::init((
        std::env::var("SENTRY_DSN")?,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            sample_rate: 1.0,
            ..Default::default()
        },
    ));

    tracing_subscriber::fmt::init();

    if !check_magick() {
        error!("Couldn't find imagemagick! exiting...");
    }
    if let Err(why) = create_tmp_dir() {
        error!("Couldn't create tmp dir: {why}");
        std::process::exit(1);
    }
    if let Err(why) = clear_tmp_dir() {
        error!("Couldn't create tmp dir: {why}");
        std::process::exit(1);
    }

    let database = libsql::Builder::new_remote(
        std::env::var("DATABASE_URL").expect("Database URL not set"),
        std::env::var("DATABASE_TOKEN").expect("Database Token not set"),
    )
    .build()
    .await?;

    let should_stop = Arc::new(AtomicBool::new(false));
    let st1 = should_stop.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        info!("Shutting down.");
        st1.store(true, Ordering::Relaxed);
    });

    loop {
        let db_conn = database.connect()?;
        let start = Instant::now();
        if should_stop.load(Ordering::Relaxed) {
            break;
        }

        let runner = runner(&db_conn, should_stop.clone());
        if let Err(why) = runner.await {
            error!("{why:#?}");
        }

        let runner_time = Instant::now() - start;

        tokio::time::sleep(
            Duration::from_secs(5)
                .checked_sub(runner_time)
                .unwrap_or(Duration::from_secs(1)),
        )
        .await;
    }

    Ok(())
}
