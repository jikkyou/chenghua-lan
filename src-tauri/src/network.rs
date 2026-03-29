use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::UdpSocket;
use std::path::PathBuf;
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
    #[serde(rename = "file_accept")]
    FileAccept {
        id: String,
        from: String,
        to: String,
    },
    #[serde(rename = "file_reject")]
    FileReject {
        id: String,
        from: String,
        to: String,
    },
    #[serde(rename = "offline")]
    Offline {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferRequest {
    pub id: String,
    pub from: String,
    pub from_name: String,
    pub to: String,
    pub file_name: String,
    pub file_size: u64,
    pub timestamp: u64,
}

pub struct NetworkState {
    pub devices: Arc<Mutex<HashMap<String, Device>>>,
    pub device_id: String,
    pub device_name: String,
    pub auto_receive: Arc<Mutex<bool>>,
    pub receive_folder: Arc<Mutex<String>>,
    pub pending_files: Arc<Mutex<HashMap<String, FileTransferRequest>>>,
}

impl NetworkState {
    pub fn new() -> Self {
        let id = Uuid::new_v4().to_string();
        let name = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| format!("设备-{}", &id[..8]));

        let receive_folder = dirs::download_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "C:\\Downloads".to_string());

        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
            device_id: id,
            device_name: name,
            auto_receive: Arc::new(Mutex::new(false)),
            receive_folder: Arc::new(Mutex::new(receive_folder)),
            pending_files: Arc::new(Mutex::new(HashMap::new())),
        }
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

pub async fn start_message_server(state: Arc<Mutex<NetworkState>>, app_handle: AppHandle) {
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
                let state = state.clone();
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
                                        NetworkMessage::File { id, from, from_name, to: _, file_name, file_size, timestamp } => {
                                            log::info!("收到文件请求: {} from {}", file_name, from_name);
                                            
                                            let file_id = id.clone();
                                            let file_from = from.clone();
                                            let file_from_name = from_name.clone();
                                            
                                            // 检查是否自动接收
                                            let auto_receive = state.lock().unwrap().auto_receive.lock().unwrap().clone();
                                            
                                            if auto_receive {
                                                // 自动接收，发送接受响应
                                                let accept_msg = NetworkMessage::FileAccept {
                                                    id: file_id.clone(),
                                                    from: state.lock().unwrap().device_id.clone(),
                                                    to: file_from.clone(),
                                                };
                                                if let Ok(json) = serde_json::to_string(&accept_msg) {
                                                    let addr = format!("{}:{}", get_local_ip(file_from.clone()).unwrap_or_default(), FILE_PORT);
                                                    if let Ok(mut stream) = TcpStream::connect(&addr).await {
                                                        let _ = stream.write_all(json.as_bytes()).await;
                                                    }
                                                }
                                                
                                                // 通知前端开始接收文件
                                                let _ = app.emit("file-receiving", serde_json::json!({
                                                    "id": file_id,
                                                    "from": file_from,
                                                    "fromName": file_from_name,
                                                    "fileName": file_name,
                                                    "fileSize": file_size,
                                                    "timestamp": timestamp,
                                                }));
                                            } else {
                                                // 存储待处理的文件请求
                                                {
                                                    state.lock().unwrap().pending_files.lock().unwrap().insert(file_id.clone(), FileTransferRequest {
                                                        id: file_id.clone(),
                                                        from: file_from.clone(),
                                                        from_name: file_from_name.clone(),
                                                        to: state.lock().unwrap().device_id.clone(),
                                                        file_name: file_name.clone(),
                                                        file_size,
                                                        timestamp,
                                                    });
                                                }
                                                
                                                // 通知前端有文件请求
                                                let _ = app.emit("file-request", serde_json::json!({
                                                    "id": file_id,
                                                    "from": file_from,
                                                    "fromName": file_from_name,
                                                    "fileName": file_name,
                                                    "fileSize": file_size,
                                                    "timestamp": timestamp,
                                                }));
                                            }
                                        }
                                        NetworkMessage::FileAccept { id, from, to: _ } => {
                                            let _ = app.emit("file-accepted", serde_json::json!({
                                                "id": id,
                                                "from": from,
                                            }));
                                        }
                                        NetworkMessage::FileReject { id, from, to: _ } => {
                                            let _ = app.emit("file-rejected", serde_json::json!({
                                                "id": id,
                                                "from": from,
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

pub async fn start_file_server(state: Arc<Mutex<NetworkState>>, app_handle: AppHandle) {
    let listener = match TcpListener::bind(format!("0.0.0.0:{}", FILE_PORT)).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("绑定文件端口失败: {}", e);
            return;
        }
    };
    log::info!("文件服务器已启动，端口: {}", FILE_PORT);

    loop {
        match listener.accept().await {
            Ok((mut stream, _from)) => {
                let app = app_handle.clone();
                let state = state.clone();
                tokio::spawn(async move {
                    // 先读取文件元数据（固定1024字节）
                    let mut meta_buf = [0u8; 1024];
                    match stream.read(&mut meta_buf).await {
                        Ok(_len) => {
                            if let Ok(json_str) = std::str::from_utf8(&meta_buf) {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                                    let id = json["id"].as_str().unwrap_or("").to_string();
                                    let from = json["from"].as_str().unwrap_or("").to_string();
                                    let file_name = json["file_name"].as_str().unwrap_or("unknown").to_string();
                                    
                                    // 读取文件内容
                                    let mut file_data = Vec::new();
                                    stream.read_to_end(&mut file_data).await.ok();
                                    
                                    // 保存文件
                                    let receive_folder = state.lock().unwrap().receive_folder.lock().unwrap().clone();
                                    let save_path = PathBuf::from(&receive_folder).join(&file_name);
                                    
                                    match fs::write(&save_path, &file_data) {
                                        Ok(_) => {
                                            log::info!("文件接收成功: {}", save_path.display());
                                            let _ = app.emit("file-received", serde_json::json!({
                                                "id": id,
                                                "from": from,
                                                "fileName": file_name,
                                                "savePath": save_path.display().to_string(),
                                            }));
                                        }
                                        Err(e) => {
                                            log::error!("文件保存失败: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("读取文件元数据失败: {}", e);
                        }
                    }
                });
            }
            Err(e) => {
                log::error!("接受文件连接失败: {}", e);
            }
        }
    }
}

fn get_local_ip(target_ip: String) -> Option<String> {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return None,
    };
    match socket.connect(format!("{}:12345", target_ip)) {
        Ok(_) => {}
        Err(_) => return None,
    }
    match socket.local_addr() {
        Ok(addr) => Some(addr.ip().to_string()),
        Err(_) => None,
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

pub async fn send_file_request(to_ip: String, file_name: String, file_size: u64, from_id: String, from_name: String, to_id: String) -> Result<String, String> {
    let file_id = Uuid::new_v4().to_string();
    let msg = NetworkMessage::File {
        id: file_id.clone(),
        from: from_id,
        from_name,
        to: to_id,
        file_name: file_name.clone(),
        file_size,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };
    
    let json = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
    let addr = format!("{}:{}", to_ip, MESSAGE_PORT);
    
    let mut stream = TcpStream::connect(&addr).await.map_err(|e| e.to_string())?;
    stream.write_all(json.as_bytes()).await.map_err(|e| e.to_string())?;
    
    Ok(file_id)
}

pub async fn send_file(to_ip: String, file_id: String, from_id: String, file_name: String, file_data: Vec<u8>) -> Result<(), String> {
    let file_size = file_data.len();
    let json = serde_json::json!({
        "type": "file",
        "id": file_id,
        "from": from_id,
        "from_name": "",
        "to": "",
        "file_name": file_name,
        "file_size": file_size,
        "timestamp": 0
    }).to_string();
    
    let addr = format!("{}:{}", to_ip, FILE_PORT);
    
    let mut stream = TcpStream::connect(&addr).await.map_err(|e| e.to_string())?;
    
    // 先发送元数据
    stream.write_all(json.as_bytes()).await.map_err(|e| e.to_string())?;
    
    // 再发送文件内容
    stream.write_all(&file_data).await.map_err(|e| e.to_string())?;
    
    Ok(())
}

pub async fn respond_file_accept(to_ip: String, file_id: String, from_id: String, to_id: String) -> Result<(), String> {
    let msg = NetworkMessage::FileAccept {
        id: file_id,
        from: from_id,
        to: to_id,
    };
    
    let json = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
    let addr = format!("{}:{}", to_ip, MESSAGE_PORT);
    
    let mut stream = TcpStream::connect(&addr).await.map_err(|e| e.to_string())?;
    stream.write_all(json.as_bytes()).await.map_err(|e| e.to_string())?;
    
    Ok(())
}

pub async fn respond_file_reject(to_ip: String, file_id: String, from_id: String, to_id: String) -> Result<(), String> {
    let msg = NetworkMessage::FileReject {
        id: file_id,
        from: from_id,
        to: to_id,
    };
    
    let json = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
    let addr = format!("{}:{}", to_ip, MESSAGE_PORT);
    
    let mut stream = TcpStream::connect(&addr).await.map_err(|e| e.to_string())?;
    stream.write_all(json.as_bytes()).await.map_err(|e| e.to_string())?;
    
    Ok(())
}
