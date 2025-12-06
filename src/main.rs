use bollard::Docker;
use std::collections::HashMap;
//use bytes::Bytes;
use futures::StreamExt;
//use futures_util::future::FutureExt;
//use log::{error, info};
use bollard::models::{
    ContainerSummaryHostConfig, ContainerSummaryNetworkSettings, ContainerSummaryStateEnum,
    MountPoint, OciDescriptor, Port,
};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::mem::take;
use std::ops::Deref;

const JETAGENT: &str = "/etc/jet-agent/agent.json";
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open(JETAGENT).expect("Unable to open config file");
    let log_data = log4rs::init_file("agent_logs.yml", Default::default());
    match log_data {
        Ok(_) => {}
        Err(err) => {
            info!("{}", err);
            panic!("{err}");
        }
    }
    let mut buf_reader = std::io::BufReader::new(file);
    let mut json_config = String::new();
    match buf_reader.read_to_string(&mut json_config) {
        Ok(_) => {}
        Err(d) => {
            panic!("{} ", d);
        }
    }
    let config_data: AgentConfig =
        serde_json::from_str(&json_config).expect("Can't read from config");
    let nats_options = async_nats::ConnectOptions::new()
        .require_tls(true)
        .add_root_certificates(config_data.security.root_certificate.clone().into())
        .add_client_certificate(
            config_data.security.certificate_file.clone().into(),
            config_data.security.key_file.clone().into(),
        );
    let docker_agent = match Docker::connect_with_socket_defaults() {
        Ok(docker) => docker,
        Err(e) => {
            error!("{}", e);
            panic!("{e}");
        }
    };
    let docker_info = list_containers(&docker_agent).await;
    let current_containers = ContainerSummary {
        containers: docker_info,
    };
    let containers_json = match serde_json::to_string(&current_containers) {
        Ok(json) => json,
        Err(err) => {
            panic!("{err}");
        }
    };
    println!("{:#?}", containers_json);
    Ok(())
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct AgentConfig {
    identity: Identity,
    networking: Networking,
    security: SecurityCerts,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
struct Identity {
    agent_id: String,
    server_uuid: String,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
struct Networking {
    nats_server: String,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
struct SecurityCerts {
    root_certificate: String,
    certificate_file: String,
    key_file: String,
}
#[derive(Serialize, Deserialize, Debug)]
struct JetTalk {
    command: String,
    args: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ContainerSummary {
    containers: Vec<bollard_stubs::models::ContainerSummary>,
}
async fn daemon_mode(
    docker_agent: &Docker,
    config_data: AgentConfig,
    nats_options: async_nats::ConnectOptions,
) {
    let client = match async_nats::connect_with_options(
        config_data.networking.nats_server.clone(),
        nats_options,
    )
    .await
    {
        Ok(c) => c,
        Err(err) => panic!("{err}"),
    };
    let mut subscriber = client
        .subscribe(format!("dockerd-{}", config_data.identity.agent_id))
        .await
        .unwrap();
    while let Some(message) = subscriber.next().await {
        let s = match std::str::from_utf8(message.payload.deref()) {
            Ok(v) => v,
            Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
        };
        let data: JetTalk = match serde_json::from_str(s) {
            Ok(s) => s,
            Err(err) => panic!("{err}"),
        };
        /*match data.command.as_str() {
            "list-docker" => {
              if let docker_info = match list_containers(docker_agent).await {
                  Ok(data) => { data }
                  Err(err) => {
                      panic!("{}", err);
                  }
              }
            "delete" => {}
            "create-storage" => {}
            "restart" => {}
            "stop" => {}
            "add-description" => {}
            "add-floating-ip" => {}
            "list-storage" => {}
            "instance" => {}
            "list" => {

                }

            _ => {}
        }*/
    }
}

async fn list_containers(docker: &Docker) -> Vec<bollard_stubs::models::ContainerSummary> {
    let mut filter = HashMap::new();
    filter.insert(
        String::from("status"),
        vec![
            String::from("exited"),
            String::from("running"),
            String::from("created"),
            String::from("removing"),
            String::from("paused"),
            String::from("dead"),
        ],
    );
    match docker
        .list_containers(Some(
            bollard::query_parameters::ListContainersOptionsBuilder::default()
                .all(true)
                .filters(&filter)
                .build(),
        ))
        .await
    {
        Ok(c) => c,
        Err(err) => {
            panic!("{err}");
        }
    }
}
