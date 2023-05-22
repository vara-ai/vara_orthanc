use reqwest::blocking::Client;
use reqwest::Result;
use serde::Serialize;
use serde_json as json;

#[derive(Debug)]
pub struct OrthancClient {
    pub url: String,
    pub username: String,
    pub password: String,
    pub http_client: Client,
}

fn get_entities(
    http_client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> Result<Vec<String>> {
    let response = http_client
        .get(url)
        .basic_auth(&username, Some(&password))
        .send()?;
    let response: json::Value = response.json()?;
    let mut ids = vec![];
    for id in response.as_array().unwrap() {
        ids.push(id.as_str().unwrap().to_string());
    }
    Ok(ids)
}

impl OrthancClient {
    pub fn get_study_ids(self: &Self) -> Result<Vec<String>> {
        let url = format!("{}/studies", self.url);
        get_entities(&self.http_client, &url, &self.username, &self.password)
    }

    pub fn transfer_studies(
        self: &Self,
        peer_identifier: &str,
        study_ids: Vec<String>,
    ) -> Result<()> {
        #[derive(Serialize, Debug)]
        struct PeerStoreRequest {
            #[serde(rename = "Asynchronous")]
            asynchronous: bool,
            #[serde(rename = "Resources")]
            resources: Vec<String>,
        }

        let request = PeerStoreRequest {
            asynchronous: false,
            resources: study_ids,
        };

        let request = self
            .http_client
            .post(format!("{}/peers/{}/store", self.url, peer_identifier))
            .basic_auth(&self.username, Some(&self.password))
            .json(&request)
            .build()?;
        let response = self.http_client.execute(request);
        if response.is_err() || !response.unwrap().status().is_success() {
            println!("Error while transferring study. Try dbg! on response above.");
        };

        Ok(())
    }
}
