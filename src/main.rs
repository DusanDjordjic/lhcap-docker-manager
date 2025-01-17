use anyhow::bail;
use clap::Parser;
use tracing::{debug, error, info, warn};

const CHECK_SLAVES_INTERVAL: u64 = 10;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    slave_image_hash: String,
    ib_image_hash: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    // Create http server that accepts messages from apps in docker
    // channel for comunicating back to the docker
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    let mut check_slaves_signal =
        tokio::time::interval(std::time::Duration::from_secs(CHECK_SLAVES_INTERVAL));

    tokio::spawn(docker_manager::server::run(tx));

    let docker = match bollard::Docker::connect_with_unix(
        "/run/docker.sock",
        128,
        bollard::API_DEFAULT_VERSION,
    ) {
        Ok(docker) => docker,
        Err(err) => {
            anyhow::bail!("failed to connect to docker, {}", err);
        }
    };

    docker.ping().await.expect("failed to ping docker server");

    let images = match docker.list_images::<&str>(None).await {
        Ok(containers) => containers,
        Err(err) => {
            anyhow::bail!("failed to list containers, {}", err);
        }
    };

    // Verify that we have slave hash and ib api hash
    let mut slave_image_hash_found = false;
    let mut ibapi_image_hash_found = false;
    for image in images {
        let image_hash = match image_extract_hash_from_id(&image.id) {
            Some(image_id) => image_id,
            None => {
                warn!(
                    image_id = image.id,
                    "failed to extract hash from image id from string"
                );
                continue;
            }
        };

        debug!("{}", image_hash);
        if image_hash.starts_with(&args.slave_image_hash) {
            slave_image_hash_found = true;
            info!(tags=?image.repo_tags, "found slave image with tags");
            continue;
        }

        if image_hash.starts_with(&args.ib_image_hash) {
            ibapi_image_hash_found = true;
            info!(tags=?image.repo_tags, "found ib api image with tags");
            continue;
        }
    }

    if !slave_image_hash_found {
        error!(
            prefix = args.slave_image_hash,
            "failed to find slave docker image with prefix"
        );

        bail!(
            "failed to find slave docker image with prefix \"{}\"",
            args.slave_image_hash
        );
    }

    if !ibapi_image_hash_found {
        error!(
            prefix = args.ib_image_hash,
            "failed to find ib api docker image with prefix"
        );

        bail!(
            "failed to find ib api docker image with prefix \"{}\"",
            args.ib_image_hash
        );
    }

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("user pressed ctrl+c");
                break
            }

            slave_message = rx.recv() => {
                info!(message=?slave_message, "slave message");
            }

            _ = check_slaves_signal.tick() => {
                info!("check slaves signal ticked");
            }
        }
    }

    Ok(())
}

fn image_extract_hash_from_id(id: &str) -> Option<&str> {
    let mut splits = id.split(":");

    if splits.next().is_none() {
        return None;
    }

    match splits.next() {
        Some(hash) => Some(hash),
        None => None,
    }
}
