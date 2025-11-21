use bollard::Docker;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::io::Read;

const JETAGENT: &str = "/etc/jet-agent/agent.yaml";
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

    Docker::connect_with_socket_defaults();
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
        .subscribe(config_data.identity.agent_id.clone())
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
            "create" => {
                let flav = get_flavor(get_flav_command(data.args[1].clone()));
                match create_fc_instance(
                    config_data.clone(),
                    data.args[0].as_str(),
                    flav,
                    sql_pool.clone(),
                    data.args[2].as_str(),
                )
                .await
                {
                    Ok(_) => {}
                    Err(err) => {
                        error!("{err}");
                    }
                }
            }
            "delete" => if delete_fc_instance(sql_pool.clone(), data.args[0].as_str()).await {},
            "create-storage" => {
                persistent_storage::cmd_create_persistent_storage(
                    config_data.clone(),
                    data.args[0].as_str(),
                    data.args[1].as_str(),
                    sql_pool.clone(),
                    data.args[2].to_string(),
                )
                .await
                .expect("");
            }
            "restart" => {
                restart_instance(
                    config_data.clone(),
                    data.args[0].to_string(),
                    get_flavor(get_flav_command(data.args[1].clone())),
                    sql_pool.clone(),
                )
                .await;
            }
            "stop" => {
                stop_fc_instance(data.args[0].to_string(), sql_pool.clone()).await;
            }
            "add-description" => {
                add_instance_description(
                    sql_pool.clone(),
                    data.args[0].to_string(),
                    data.args[1].to_string(),
                )
                .await;
            }
            "add-floating-ip" => {
                remove_nft(sql_pool.clone(), data.args[0].clone()).await;
                add_floating_ip(sql_pool.clone(), data.args[0].clone(), data.args[1].clone()).await;
            }
            "list-storage" => {
                match persistent_storage::cmd_list_persistent_storage(sql_pool.clone()).await {
                    Ok(data) => {
                        if let Some(reply_subject) = message.reply {
                            let json = match serde_json::to_string(&data) {
                                Ok(json) => json,
                                Err(err) => {
                                    error!("{err}");
                                    return;
                                }
                            };
                            println!("{}", json);
                            match client.publish(reply_subject, json.into()).await {
                                Ok(data) => {
                                    error!("{:?}", data);
                                }
                                Err(err) => error!("{err}"),
                            };
                            match client.flush().await {
                                Ok(_) => {}
                                Err(err) => error!("{err}"),
                            }
                        }
                    }
                    Err(err) => {
                        error!("{err}")
                    }
                }
            }
            "instance" => {
                let instance_data = get_instance(sql_pool.clone(), data.args[0].clone()).await;
                if let Some(reply_subject) = message.reply {
                    let json = match serde_json::to_string(&instance_data) {
                        Ok(json) => json,
                        Err(err) => {
                            error!("{err}");
                            return;
                        }
                    };
                    match client.publish(reply_subject, json.into()).await {
                        Ok(data) => {
                            info!("{:?}", data);
                        }
                        Err(err) => error!("{err}"),
                    };
                    match client.flush().await {
                        Ok(_) => {}
                        Err(err) => error!("{err}"),
                    }
                }
            }
            "list" => {
                let data_list = list_fc_instances_internal(sql_pool.clone()).await;
                let mut liststatus: Vec<MiniFcE> = Vec::new();
                for data in data_list {
                    let current = MiniFcE {
                        fc_instance_name: data.fc_instance_name.clone(),
                        fc_socket_location: data.fc_socket_location.clone(),
                        fc_process_id: data.fc_process_id.clone(),
                        fc_chroot_dir: data.fc_chroot_dir.clone(),
                        fc_container_id: data.fc_container_id.clone(),
                        fc_ip_address: data.ip_address.clone(),
                        fc_route_ip_address: data.route_ip_address.clone(),
                        fc_tap_device: data.tap_device_name.clone(),
                        fc_vcpu: data.fc_vcpu.clone(),
                        fc_memory: data.fc_memory.clone(),
                        fc_description: data.fc_description.clone(),
                        fc_floating_ip: data.public_ip.clone(),
                        fc_created: data.fc_created.clone(),
                        fc_status: Path::new(format!("/proc/{}/", data.fc_process_id).as_str())
                            .exists(),
                    };
                    liststatus.push(current.clone())
                }
                let json = match serde_json::to_string(&liststatus) {
                    Ok(json) => json,
                    Err(err) => {
                        error!("{err}");
                        return;
                    }
                };
                println!("{}", json);
                if let Some(reply_subject) = message.reply {
                    match client.publish(reply_subject, json.into()).await {
                        Ok(data) => {
                            info!("{:?}", data);
                        }
                        Err(err) => error!("{err}"),
                    };
                    match client.flush().await {
                        Ok(_) => {}
                        Err(err) => error!("{err}"),
                    }
                }
            }
            _ => {}
        }
    }
}
