use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

const DISCOVERY_PORT: u16 = 19876;
const MESSAGE_PORT: u16 = 19877;
const FILE_PORT: u16 = 19878;
const BROADCAST_ADDR: &'static str = "255.255.255.255";
const HEARTBEAT_INTERVAL: u64 = 3;
const DEVICE_TIMEOUT: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub online: bool,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NetworkMessage {
    #[serde(rename = "heartbeat")]
    Heartbeat {
        id: String,
        name: String,
    },
    #[serde(rename = "message")]
    Message {
        id: String,
        from: String,
        from_name: String,
        to: String,
        content: String,
        timestamp: u64,
    },
    #[serde(rename = "file")]
    File {
        id: String,
        from: String,
        from_name: String,
        to: String,
        file_name: String,
        file_size: u64,
        timestamp: u64,
    },
    #[serde(rename = "offline")]
    Offline {
        id: String,
    },
}

pub struct NetworkState {
    pub devices: Arc<Mutex<HashMap<String, Device>>>,
    pub device_id: String,
    pub device_name: String,
}

impl NetworkState {
    pub fn new() -> Self {
        let id = Uuid::new_v4().to_string();
        let name = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| format!("设备-{}", &id[..8]));

        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
            device_id: id,
            device_name: name,
        }
    }
}

pub fn get_local_ip() -> Option<String> {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return None,
    };
    match socket.connect("8.8.8.8:80") {
        Ok(_) => {}
        Err(_) => return None,
    }
    match socket.local_addr() {
        Ok(addr) => Some(addr.ip().to_string()),
        Err(_) => None,
    }
}

pub async fn start_discovery(state: Arc<Mutex<NetworkState>>, app_handle: AppHandle) {
    let socket = match UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)) {
        Ok(s) => s,
        Err(e) => {
            log::error!("绑定UDP端口失败: {}", e);
            return;
        }
    };

    if let Err(e) = socket.set_broadcast(true) {
        log::error!("设置广播失败: {}", e);
        return;
    }

    socket.set_read_timeout(Some(Duration::from_secs(1))).ok();

    let device_id = state.lock().unwrap().device_id.clone();
    let device_name = state.lock().unwrap().device_name.clone();

    log::info!("设备ID: {}, 名称: {}", device_id, device_name);

    let broadcast_socket = socket.try_clone().unwrap();
    tokio::spawn(async move {
        loop {
            let msg = NetworkMessage::Heartbeat {
                id: device_id.clone(),
                name: device_name.clone(),
            };
            if let Ok(json) = serde_json::to_string(&msg) {
                let addr = format!("{}:{}", BROADCAST_ADDR, DISCOVERY_PORT);
                let _ = broadcast_socket.send_to(json.as_bytes(), &addr);
            }
            tokio::time::sleep(Duration::from_secs(HEARTBEAT_INTERVAL)).await;
        }
    });

    let listen_state = state.clone();
    let app = app_handle.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((len, from)) => {
                    if let Ok(json) = std::str::from_utf8(&buf[..len]) {
                        if let Ok(msg) = serde_json::from_str::<NetworkMessage>(json) {
                            match msg {
                                NetworkMessage::Heartbeat { id, name } => {
                                    if id != listen_state.lock().unwrap().device_id {
                                        let ip = from.ip().to_string();
                                        {
                                            let mut devices = listen_state.lock().unwrap();
                                            let is_new = !devices.devices.lock().unwrap().contains_key(&id);
                                            if is_new {
                                                log::info!("发现新设备: {} ({})", name, ip);
                                            }
                                            
                                            devices.devices.lock().unwrap().insert(id.clone(), Device {
                                                id: id.clone(),
                                                name: name.clone(),
                                                ip: ip.clone(),
                                                online: true,
                                                last_seen: std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .unwrap()
                                                    .as_secs(),
                                            });
                                        }
                                        
                                        let devs: Vec<Device> = listen_state.lock().unwrap().devices.lock().unwrap().values().cloned().collect();
                                        let _ = app.emit("devices-changed", devs);
                                    }
                                }
                                NetworkMessage::Offline { id } => {
                                    let did_update = {
                                        let devices = listen_state.lock().unwrap();
                                        let mut inner = devices.devices.lock().unwrap();
                                        if let Some(device) = inner.get_mut(&id) {
                                            device.online = false;
                                            log::info!("设备离线: {}", device.name);
                                            true
                                        } else {
                                            false
                                        }
                                    };
                                    if did_update {
                                        let devs: Vec<Device> = listen_state.lock().unwrap().devices.lock().unwrap().values().cloned().collect();
                                        let _ = app.emit("devices-changed", devs);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(_) => {
                    {
                        let mut devices = listen_state.lock().unwrap();
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        let mut changed = false;
                        
                        devices.devices.lock().unwrap().retain(|_, v| {
                            if now - v.last_seen > DEVICE_TIMEOUT {
                                changed = true;
                                false
                            } else {
                                true
                            }
                        });
                        
                        if changed {
                            let devs: Vec<Device> = devices.devices.lock().unwrap().values().cloned().collect();
                            drop(devices);
                            let _ = app.emit("devices-changed", devs);
                        }
                    }
                }
            }
        }
    });
}

pub async fn start_message_server(_state: Arc<Mutex<NetworkState>>, app_handle: AppHandle) {
    let listener = match TcpListener::bind(format!("0.0.0.0:{}", MESSAGE_PORT)).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("绑定消息端口失败: {}", e);
            return;
        }
    };
    log::info!("消息服务器已启动，端口: {}", MESSAGE_PORT);

    loop {
        match listener.accept().await {
            Ok((mut stream, _from)) => {
                let app = app_handle.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 8192];
                    match stream.read(&mut buf).await {
                        Ok(len) => {
                            if let Ok(json) = std::str::from_utf8(&buf[..len]) {
                                if let Ok(msg) = serde_json::from_str::<NetworkMessage>(json) {
                                    match msg {
                                        NetworkMessage::Message { id, from, from_name, to, content, timestamp } => {
                                            log::info!("收到消息 from {}: {}", from_name, content);
                                            let _ = app.emit("message-received", serde_json::json!({
                                                "id": id,
                                                "from": from,
                                                "fromName": from_name,
                                                "to": to,
                                                "content": content,
                                                "timestamp": timestamp,
                                                "type": "message"
                                            }));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("读取消息失败: {}", e);
                        }
                    }
                });
            }
            Err(e) => {
                log::error!("接受连接失败: {}", e);
            }
        }
    }
}

pub async fn send_message(to_ip: String, from_id: String, from_name: String, to_id: String, content: String) -> Result<(), String> {
    let msg = NetworkMessage::Message {
        id: Uuid::new_v4().to_string(),
        from: from_id,
        from_name,
        to: to_id,
        content,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };
    
    let json = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
    let addr = format!("{}:{}", to_ip, MESSAGE_PORT);
    
    let mut stream = TcpStream::connect(&addr).await.map_err(|e| e.to_string())?;
    stream.write_all(json.as_bytes()).await.map_err(|e| e.to_string())?;
    
    Ok(())
}
