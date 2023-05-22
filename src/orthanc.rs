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
        write!(f, "Url: {} Username: {} Password: {}", self.url, self.username, self.password)
    }
}

impl OrthancClient {
    pub fn new(url: &str, username: &str, password: &str) -> Self {
        let http_client = reqwest::blocking::Client::new();
        OrthancClient {
            url: String::from(url),
            username: String::from(username),
            password: String::from(password),
            http_client,
        }
    }
}

pub fn sync_studies() {
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

    let local_studies: HashSet<String> =
        local_orthanc.get_study_ids().unwrap().into_iter().collect();
    let peer_studies: HashSet<String> = peer_orthanc.get_study_ids().unwrap().into_iter().collect();
    let missing_studies: Vec<String> = local_studies
        .into_iter()
        .filter(|local_study_id| peer_studies.contains(local_study_id))
        .collect();
    match local_orthanc.transfer_studies(&plugin::get_peer_identifier(), missing_studies) {
        Ok(response) => plugin::info(&format!("Successfully transferred studies: {:?}", response)),
        Err(error) => plugin::info(&format!("Failed to transfer studies: {:?}", error))
    };
}
