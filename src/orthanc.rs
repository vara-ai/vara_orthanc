use reqwest::Result;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Display;
pub mod http;
pub mod plugin;

pub use http::OrthancClient;

#[derive(Debug)]
pub struct Endpoint {
    pub url: String,
    pub username: String,
    pub password: String,
}

impl Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Url: {} Username: {}", self.url, self.username)
    }
}

impl OrthancClient {
    pub fn new(url: &str, username: &str, password: &str) -> Self {
        // Cloning a `reqwest::blocking::Client` creates a new handle to same
        // client.
        let http_client = plugin::PLUGIN_STATE
            .read()
            .unwrap()
            .http_client
            .clone()
            .unwrap();
        OrthancClient {
            url: String::from(url),
            username: String::from(username),
            password: String::from(password),
            http_client,
        }
    }
}

pub fn sync_instances() -> Result<()> {
    let local_endpoint = plugin::get_local_endpoint();
    let peer_endpoint = plugin::get_peer_endpoint().unwrap();
    plugin::info(&format!(
        "Synchronizing studies between: {} -> {}",
        local_endpoint, peer_endpoint
    ));

    let local_orthanc = OrthancClient::new(
        &local_endpoint.url,
        &local_endpoint.username,
        &local_endpoint.password,
    );

    let peer_orthanc = OrthancClient::new(
        &peer_endpoint.url,
        &peer_endpoint.username,
        &peer_endpoint.password,
    );

    let local_instances: HashSet<String> = local_orthanc.get_instance_ids()?.into_iter().collect();
    let peer_instances: HashSet<String> = peer_orthanc.get_instance_ids()?.into_iter().collect();

    // plugin::info(&format!(
    //     "Local: {:?}, Remote: {:?}",
    //     &local_studies, &peer_studies
    // ));

    let missing_instances: Vec<String> = local_instances
        .into_iter()
        .filter(|local_study_id| !peer_instances.contains(local_study_id))
        .collect();
    if !missing_instances.is_empty() {
        plugin::info(&format!("Transferring instances: {:?}", &missing_instances));
        match local_orthanc.transfer_instances(&plugin::get_peer_identifier(), missing_instances) {
            Ok(_response) => plugin::info(&format!("Successfully transferred instances")),
            Err(error) => {
                plugin::info(&format!("Failed to transfer studies: {:?}", error));
                return Err(error);
            }
        }
    } else {
        plugin::info("No new studies to sync.");
    }

    Ok(())
}
