use bollard::Docker;
//use bytes::Bytes;
use futures::StreamExt;
//use futures_util::future::FutureExt;
//use log::{error, info};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::ops::Deref;

const JETAGENT: &str = "/etc/jet-agent/agent.json";
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open(JETAGENT).expect("Unable to open config file");
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
        Err(e) => panic!("{}", e),
    };
    let version = docker_agent.info().await.unwrap();
    println!("{:?}", version);
    daemon_mode(config_data, nats_options).await;
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
async fn daemon_mode(config_data: AgentConfig, nats_options: async_nats::ConnectOptions) {
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
        match data.command.as_str() {
            "create" => {}
            "delete" => {}
            "create-storage" => {}
            "restart" => {}
            "stop" => {}
            "add-description" => {}
            "add-floating-ip" => {}
            "list-storage" => {}
            "instance" => {}
            "list" => {}
            _ => {}
        }
    }
}
