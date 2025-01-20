use anyhow::bail;
use bollard::{container::ListContainersOptions, Docker};
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

    let mut slave_container_manager = ContainerManager::new(args.slave_image_hash, 5);
    let mut ib_instance_count = ContainerManager::new(args.ib_image_hash, 1);

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
                // check how many instances of rust program we have
                if let Err(err) = check_instances(&docker, &mut slave_container_manager).await {
                    // handle error
                }
                if let Err(err) = check_instances(&docker, &mut ib_instance_count).await {
                    // handle error
                }
            }
        }
    }

    Ok(())
}

async fn check_instances(
    docker: &Docker,
    container_manager: &ContainerManager,
) -> anyhow::Result<()> {
    let containers = match docker.list_containers::<String>(None).await {
        Ok(containers) => containers,
        Err(err) => bail!("failed to get containers, {}", err),
    };

    let image_hash = container_manager.get_image_hash();

    for container in containers {
        println!("{:?}", container.image_id);
        match container.image_id {
            Some(v) => {}
            None => {
                warn!("failed to get image_id");
            }
        }

        // Check if the image_id matches the image_hash and how many running instances we have
    }

    Ok(())
}
pub struct ContainerManager {
    target_count: u16,
    current_count: u16,
    image_hash: String,
}

impl ContainerManager {
    pub fn new(image_hash: String, target: u16) -> Self {
        Self {
            image_hash,
            target_count: target,
            current_count: 0,
        }
    }

    pub fn get_image_hash(&self) -> &str {
        self.image_hash.as_str()
    }

    pub fn get_target(&self) -> u16 {
        self.target_count
    }

    pub fn get_current(&self) -> u16 {
        self.current_count
    }

    pub fn set_current(&mut self, curr: u16) -> u16 {
        self.current_count = curr;
        self.current_count
    }
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
